import dataclasses
import enum
import json
import unittest
from types import SimpleNamespace as NS

from wardscript import Partial, _rt, runtime
from wardscript.partial import MISSING, decode_partial, parse_prefix
from wardscript.providers.anthropic import Anthropic


class Priority(enum.Enum):
    Low = "Low"
    Urgent = "Urgent"


@dataclasses.dataclass(frozen=True)
class Ticket:
    customer: str
    priority: Priority
    tags: list
    order: object


_rt.enum(Priority, "Priority", lambda: [("Low", Priority.Low, []), ("Urgent", Priority.Urgent, [])])
_rt.record(
    Ticket,
    "Ticket",
    lambda: [
        ("customer", "customer", _rt.String),
        ("priority", "priority", _rt.Adt(Priority)),
        ("tags", "tags", _rt.List(_rt.String)),
        ("order", "order", _rt.Option(_rt.Int)),
    ],
)
TICKET = _rt.Adt(Ticket)
FULL = {"customer": "Ada", "priority": "Urgent", "tags": ["crash", "web"], "order": 1042}


class Prefixes(unittest.TestCase):
    def test_parse_prefix(self):
        self.assertIs(parse_prefix(""), MISSING)
        self.assertEqual(parse_prefix('{"a": "Hel'), {"a": "Hel"})
        self.assertEqual(parse_prefix('{"a": 1'), {})
        self.assertEqual(parse_prefix('{"a": 1,'), {"a": 1})
        self.assertEqual(parse_prefix('["x", "y'), ["x", "y"])
        self.assertEqual(parse_prefix('"caf\\u00e'), "caf")
        self.assertIs(parse_prefix("nul"), MISSING)

    def test_every_prefix_decodes_without_errors(self):
        text = json.dumps(FULL)
        last = None
        for n in range(len(text) + 1):
            value = decode_partial(TICKET, parse_prefix(text[:n]))
            if value is MISSING:
                continue
            last = value
        self.assertEqual(last, Ticket("Ada", Priority.Urgent, ["crash", "web"], 1042))

    def test_partial_records(self):
        p = decode_partial(TICKET, parse_prefix('{"customer": "Ad", "priority": "Urg'))
        self.assertIsInstance(p, Partial)
        self.assertEqual(p.customer, "Ad")
        self.assertNotIn("priority", p.fields)  # An enum appears only once complete.
        self.assertEqual(repr(p), "Partial(Ticket, customer='Ad')")
        p = decode_partial(TICKET, parse_prefix('{"customer": "Ada", "priority": "Urgent", "tags": ["cr'))
        self.assertEqual((p.priority, p.tags), (Priority.Urgent, ["cr"]))


class Streaming(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_on_partial_sees_the_answer_grow(self):
        class Fake:
            def __init__(self):
                self.messages = self

            def create(self, stream=False, **request):
                text = json.dumps({"value": FULL})
                events = [NS(type="message_start", message=NS(usage=NS(input_tokens=10, output_tokens=0)))]
                events += [
                    NS(type="content_block_delta", delta=NS(type="input_json_delta", partial_json=text[i : i + 1]))
                    for i in range(len(text))
                ]
                return iter(events + [NS(type="message_delta", usage=NS(output_tokens=30))])

        seen = []
        runtime.configure(model=Anthropic(client=Fake()), on_partial=seen.append)
        ticket = _rt.ai("triage", "p", TICKET)
        self.assertEqual(ticket.customer, "Ada")
        values = [s.value for s in seen]
        self.assertGreater(len(values), 5)
        self.assertEqual(values[0], Partial(Ticket, {}))
        names = [v.fields.get("customer") for v in values if isinstance(v, Partial)]
        self.assertIn("Ad", names)  # A string shows up as it's written.
        self.assertTrue(seen[-1].done)
        self.assertEqual(values[-1], ticket)
        # Each partial while streaming is new; the `done` one repeats the complete value.
        streamed = [s.value for s in seen if not s.done]
        self.assertTrue(all(a != b for a, b in zip(streamed, streamed[1:])))


if __name__ == "__main__":
    unittest.main()
