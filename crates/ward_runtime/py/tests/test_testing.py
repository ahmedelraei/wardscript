import asyncio
import io
import json
import os
import tempfile
import unittest

from wardscript import RateLimited, TestFailure, Thrown, _rt, runtime, testing
from wardscript.mock import MockModel, Seq


def ask(prompt="p"):
    return _rt.ai("f", prompt, _rt.Int)


def send(*args):
    return _rt.call_tool("mail", "send", "a.ward:1:1", *args)


class Testing(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "t.recordings.json")

    def tearDown(self):
        runtime.reset()
        self.dir.cleanup()

    def run_tests(self, tests, **kw):
        out = io.StringIO()
        ok = testing.run(tests, recordings=self.path, out=out, **kw)
        return ok, out.getvalue()

    def test_record_then_replay(self):
        sent = []

        def t():
            with _rt.call("test t", []):
                n = ask()
                send(f"got {n}")
                if n != 7:
                    raise TestFailure("n is 7", "t.ward:1:1")

        tools = {"mail": {"send": lambda body: sent.append(body) or "ok"}}
        ok, out = self.run_tests(
            [("t", "t.ward:1:1", t)], record=True, sources=["mail"], model=MockModel({"f": 7}), tools=tools
        )
        self.assertTrue(ok, out)
        self.assertEqual(sent, ["got 7"])
        with open(self.path, encoding="utf-8") as f:
            events = json.load(f)["tests"]["t"]
        self.assertEqual([e["kind"] for e in events], ["model", "tool"])
        self.assertEqual(events[1]["args"], ["got 7"])

        # The replay needs neither the model nor the tool.
        ok, out = self.run_tests([("t", "t.ward:1:1", t)], sources=["mail"])
        self.assertTrue(ok, out)
        self.assertEqual(sent, ["got 7"])
        self.assertIn("test t ... ok", out)

    def test_mismatches_fail(self):
        self.run_tests([("t", "s", lambda: ask("p"))], record=True, model=MockModel({"f": 1}))
        cases = {
            "a different prompt": lambda: ask("q"),
            "recording ends": lambda: (ask("p"), ask("p")),
            "made 0 calls": lambda: None,
        }
        for expected, body in cases.items():
            ok, out = self.run_tests([("t", "s", body)])
            self.assertFalse(ok)
            self.assertIn(expected, out)

    def test_model_errors_are_replayed(self):
        runtime.configure(backoff=0)
        model = MockModel({"f": Seq(RateLimited("429"), 3)})
        ok, out = self.run_tests([("t", "s", ask)], record=True, model=model)
        self.assertTrue(ok, out)
        runtime.configure(backoff=0)
        ok, out = self.run_tests([("t", "s", ask)])
        self.assertTrue(ok, out)
        with open(self.path, encoding="utf-8") as f:
            events = json.load(f)["tests"]["t"]
        self.assertEqual(events[0]["error"]["type"], "RateLimited")

    def test_thrown_tool_errors_and_named_arguments(self):
        class Server:
            def call_tool(self, name, arguments):
                raise Thrown("mailbox full")

        def t():
            try:
                _rt.call_tool("mail", "send", "a:1:1", "x", names=("body",), sinks=(True,), returns=_rt.String)
            except Thrown as e:
                if e.value != "mailbox full":
                    raise TestFailure("wrong error", "s")

        ok, out = self.run_tests([("t", "s", t)], record=True, sources=["mail"], model=MockModel({}), tools={"mail": Server()})
        self.assertTrue(ok, out)
        ok, out = self.run_tests([("t", "s", t)], sources=["mail"])
        self.assertTrue(ok, out)

    def test_failures_and_filters(self):
        def boom():
            raise Thrown("bad")

        tests = [("passes", "s", lambda: None), ("throws", "s:2:1", boom)]
        ok, out = self.run_tests(tests, record=True, model=MockModel({}))
        self.assertFalse(ok)
        self.assertIn('throws ... FAILED\n    threw "bad"', out)
        self.assertIn("1 passed, 1 failed", out)
        ok, out = self.run_tests(tests, filters=["pass"])
        self.assertTrue(ok, out)
        self.assertIn("1 filtered out", out)

    def test_async_tests(self):
        async def t():
            with _rt.call("test t", []):
                await _rt.ai_async("f", "p", _rt.Int)

        ok, out = self.run_tests([("t", "s", t)], record=True, model=MockModel({"f": 1}))
        self.assertTrue(ok, out)
        ok, out = self.run_tests([("t", "s", t)])
        self.assertTrue(ok, out)


if __name__ == "__main__":
    unittest.main()
