"""The providers against fake API clients: the request they send and how they read the
answer. `WARD_LIVE=1` tests run them against the real APIs (tests/live)."""

import json
import unittest
from types import SimpleNamespace as NS

from wardscript import _rt, runtime
from wardscript.model import AiRequest
from wardscript.providers import load, object_schema
from wardscript.providers.anthropic import Anthropic
from wardscript.providers.openai import OpenAI


class FakeAnthropic:
    def __init__(self, content):
        self.content = content
        self.requests = []
        self.messages = self

    def create(self, **request):
        self.requests.append(request)
        return NS(content=self.content, usage=NS(input_tokens=100, output_tokens=20))


class FakeOpenAI:
    def __init__(self, content):
        self.content = content
        self.requests = []
        self.chat = NS(completions=self)

    def create(self, **request):
        self.requests.append(request)
        message = NS(content=self.content)
        return NS(choices=[NS(message=message)], usage=NS(prompt_tokens=100, completion_tokens=20))


SCHEMA = {"$ref": "#/$defs/T", "$defs": {"T": {"type": "object", "properties": {"a": {"type": "integer"}}}}}


class Providers(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_object_schema_keeps_defs_at_the_top(self):
        wrapped = object_schema(SCHEMA)
        self.assertEqual(wrapped["properties"]["value"], {"$ref": "#/$defs/T"})
        self.assertIn("T", wrapped["$defs"])
        self.assertIn("$defs", SCHEMA)

    def test_anthropic(self):
        client = FakeAnthropic([NS(type="tool_use", input={"value": {"a": 1}})])
        model = Anthropic(prices=(3.0, 15.0), client=client)
        request = AiRequest("f", "the prompt", SCHEMA, 1, ("bad answer",))
        answer = model.complete(request)
        self.assertEqual(json.loads(answer.text), {"a": 1})
        self.assertEqual(answer.tokens, 120)
        self.assertAlmostEqual(answer.cost, (100 * 3 + 20 * 15) / 1e6)
        sent = client.requests[0]
        self.assertEqual(sent["model"], "claude-sonnet-5")
        self.assertEqual(sent["tool_choice"], {"type": "tool", "name": "answer"})
        self.assertEqual(sent["tools"][0]["input_schema"], object_schema(SCHEMA))
        self.assertIn("rejected: bad answer", sent["messages"][0]["content"])

    def test_anthropic_without_a_tool_call(self):
        client = FakeAnthropic([NS(type="text", text="not json")])
        self.assertEqual(Anthropic(client=client).complete(AiRequest("f", "p", SCHEMA)).text, "not json")
        self.assertEqual(Anthropic(client=client).complete(AiRequest("f", "p", SCHEMA)).cost, 0.0)

    def test_openai(self):
        client = FakeOpenAI(json.dumps({"value": [1, 2]}))
        answer = OpenAI("some-model", client=client).complete(AiRequest("f", "p", {"type": "array"}))
        self.assertEqual(json.loads(answer.text), [1, 2])
        self.assertEqual(answer.tokens, 120)
        self.assertEqual(client.requests[0]["response_format"]["type"], "json_schema")

    def test_budgets_count_reported_usage(self):
        client = FakeAnthropic([NS(type="tool_use", input={"value": 7})])
        runtime.configure(model=Anthropic(prices=(3.0, 15.0), client=client))
        with _rt.budget("f", tokens=1000) as b:
            self.assertEqual(_rt.ai("f", "p", _rt.Int), 7)
        self.assertEqual(b.used[0], 120.0)

    def test_load(self):
        with self.assertRaises(ValueError):
            load("nope")
        with self.assertRaises(ValueError):
            load("openai")


if __name__ == "__main__":
    unittest.main()
