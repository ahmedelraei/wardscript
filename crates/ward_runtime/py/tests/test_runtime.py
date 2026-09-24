import asyncio
import dataclasses
import enum
import http.server
import json
import tempfile
import threading
import unittest
import warnings

from wardscript import (
    AiOutputError,
    ApprovalDenied,
    BudgetExceeded,
    Completion,
    DecodeError,
    PanicError,
    Some,
    Thrown,
    ToolError,
    Trusted,
    TrustError,
    _rt,
    decode,
    encode,
    json_schema,
    runtime,
)
from wardscript.mock import MockError, MockModel, Raw, Seq, Usage


class Color(enum.Enum):
    Red = "Red"
    Green = "Green"


class Shape:
    pass


@dataclasses.dataclass(frozen=True)
class Shape_Dot(Shape):
    pass


@dataclasses.dataclass(frozen=True)
class Shape_Circle(Shape):
    _0: float


Shape.Dot = Shape_Dot
Shape.Circle = Shape_Circle


@dataclasses.dataclass(frozen=True)
class Page:
    items: list
    next: object


@dataclasses.dataclass(frozen=True)
class Node:
    label: str
    children: list
    from_: int


_rt.enum(Color, "Color", lambda: [("Red", Color.Red, []), ("Green", Color.Green, [])])
_rt.enum(Shape, "Shape", lambda: [("Dot", Shape.Dot, []), ("Circle", Shape.Circle, [_rt.Float])])
_rt.record(Page, "Page", lambda: [("items", "items", _rt.List(_rt.Param(0))), ("next", "next", _rt.Option(_rt.String))])
_rt.record(
    Node,
    "Node",
    lambda: [
        ("label", "label", _rt.String),
        ("children", "children", _rt.List(_rt.Adt(Node))),
        ("from", "from_", _rt.Int),
    ],
)


class Values(unittest.TestCase):
    def test_some_wraps_only_when_needed(self):
        self.assertEqual(_rt.some(3), 3)
        self.assertEqual(_rt.some(None), Some(None))
        self.assertEqual(_rt.some(Some(None)), Some(Some(None)))
        self.assertEqual(_rt.unwrap(_rt.some(None)), None)
        self.assertEqual(_rt.unwrap_or(None, 1), 1)
        self.assertEqual(_rt.unwrap_or(Some(None), 1), None)

    def test_integer_arithmetic(self):
        cases = [(7, 2, 3, 1), (-7, 2, -3, -1), (7, -2, -3, 1), (-7, -2, 3, -1)]
        for a, b, q, r in cases:
            self.assertEqual((_rt.idiv(a, b), _rt.irem(a, b)), (q, r), (a, b))
        with self.assertRaises(PanicError):
            _rt.irem(1, 0)

    def test_float_arithmetic(self):
        self.assertEqual(_rt.fdiv(1.0, 0.0), float("inf"))
        self.assertEqual(_rt.fdiv(-1.0, 0.0), float("-inf"))
        self.assertEqual(_rt.fdiv(1.0, -0.0), float("-inf"))
        self.assertNotEqual(_rt.fdiv(0.0, 0.0), _rt.fdiv(0.0, 0.0))
        self.assertEqual(_rt.frem(-7.5, 2.0), -1.5)

    def test_to_str(self):
        self.assertEqual(_rt.to_str(True), "true")
        self.assertEqual(_rt.to_str(Color.Red), "Red")
        self.assertEqual(_rt.to_str(Shape.Circle(1.5)), '{"Circle": [1.5]}')
        self.assertEqual(_rt.to_str(Some(None)), "None")

    def test_collections(self):
        self.assertEqual(_rt.list_get([1], 1), None)
        self.assertEqual(_rt.first([None]), Some(None))
        self.assertEqual(_rt.list_set([1, 2], 0, 5), [5, 2])
        with self.assertRaises(PanicError):
            _rt.map_index({}, "k")
        self.assertEqual(_rt.with_field({"a": 1}, "a", 2), {"a": 2})
        self.assertEqual(_rt.field({"a": 1}, "a"), 1)


class Schemas(unittest.TestCase):
    def test_records_enums_and_generics(self):
        schema = json_schema(_rt.Adt(Page, _rt.Adt(Color)))
        self.assertEqual(schema["$ref"], "#/$defs/Page_Color")
        page = schema["$defs"]["Page_Color"]
        self.assertEqual(page["required"], ["items", "next"])
        self.assertFalse(page["additionalProperties"])
        self.assertEqual(page["properties"]["items"], {"type": "array", "items": {"$ref": "#/$defs/Color"}})
        self.assertEqual(schema["$defs"]["Color"], {"type": "string", "enum": ["Red", "Green"]})

    def test_enums_with_fields(self):
        schema = json_schema(_rt.Adt(Shape))
        dot, circle = schema["$defs"]["Shape"]["oneOf"]
        self.assertEqual(dot, {"const": "Dot"})
        self.assertEqual(circle["properties"]["Circle"]["prefixItems"], [{"type": "number"}])

    def test_recursive_records(self):
        schema = json_schema(_rt.Adt(Node))
        self.assertEqual(schema["$defs"]["Node"]["properties"]["children"]["items"], {"$ref": "#/$defs/Node"})
        self.assertIn("from", schema["$defs"]["Node"]["properties"])

    def test_primitives(self):
        self.assertEqual(json_schema(_rt.Option(_rt.Int)), {"anyOf": [{"type": "integer"}, {"type": "null"}]})
        self.assertEqual(json_schema(_rt.Map(_rt.String, _rt.Bool)), {"type": "object", "additionalProperties": {"type": "boolean"}})


class Decoding(unittest.TestCase):
    def test_round_trip(self):
        t = _rt.Adt(Node)
        value = {"label": "a", "children": [{"label": "b", "children": [], "from": 2}], "from": 1}
        node = decode(t, value)
        self.assertEqual(node.children[0].from_, 2)
        self.assertEqual(encode(node), value)

        shapes = decode(_rt.List(_rt.Adt(Shape)), ["Dot", {"Circle": [2]}])
        self.assertEqual(shapes, [Shape.Dot(), Shape.Circle(2.0)])
        self.assertEqual(encode(shapes), ["Dot", {"Circle": [2.0]}])

    def test_errors_say_where(self):
        with self.assertRaises(DecodeError) as cm:
            decode(_rt.Adt(Node), {"label": "a", "children": [{"label": 1, "children": [], "from": 0}], "from": 0})
        self.assertEqual(cm.exception.path, "$.children[0].label")

        bad = [
            (_rt.Int, True, "expected an integer"),
            (_rt.Int, 1.5, "expected an integer"),
            (_rt.Adt(Color), "Blue", "not a variant"),
            (_rt.Adt(Shape), {"Circle": []}, "array of 1"),
            (_rt.Adt(Page, _rt.Int), {"items": []}, "missing field `next`"),
            (_rt.Adt(Page, _rt.Int), {"items": [], "next": None, "extra": 1}, "no field `extra`"),
        ]
        for t, value, message in bad:
            with self.assertRaises(DecodeError, msg=repr(value)) as cm:
                decode(t, value)
            self.assertIn(message, str(cm.exception))

    def test_nested_options(self):
        t = _rt.Option(_rt.Option(_rt.Int))
        self.assertEqual(decode(t, 3), 3)
        self.assertEqual(decode(t, None), None)
        self.assertEqual(decode(_rt.Map(_rt.Int, _rt.Int), {"1": 2}), {1: 2})


class Runtime(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_ai_retries_then_fails(self):
        model = MockModel({"f": Seq(Raw("{"), Raw('"x"'), 7)})
        runtime.configure(model=model)
        self.assertEqual(_rt.ai("f", "prompt", _rt.Int), 7)
        self.assertEqual(len(model.calls), 3)
        self.assertIn("expected an integer", model.calls[2].errors[1])
        self.assertIn("rejected", model.calls[2].instructions())

        runtime.configure(model=MockModel({"f": "no"}), retries=0)
        with self.assertRaises(AiOutputError):
            _rt.ai("f", "prompt", _rt.Int)

    def test_mock(self):
        model = MockModel.from_json(json.dumps({"f": {"items": [1], "next": None}}))
        runtime.configure(model=model)
        self.assertEqual(_rt.ai("f", "p", _rt.Adt(Page, _rt.Int)), Page(items=[1], next=None))
        runtime.configure(model=MockModel({"f": lambda request: len(request.prompt)}))
        self.assertEqual(_rt.ai("f", "four", _rt.Int), 4)
        with self.assertRaises(MockError):
            _rt.ai("g", "p", _rt.Int)
        runtime.configure(model=MockModel({"f": Seq()}))
        with self.assertRaises(MockError):
            _rt.ai("f", "p", _rt.Int)

    def test_configure_keeps_what_is_not_given(self):
        runtime.configure(retries=5)
        runtime.configure(tools={})
        self.assertEqual(runtime.config().retries, 5)
        with self.assertRaises(ValueError):
            runtime.configure(retries=-1)

    def test_approve(self):
        with self.assertRaises(ApprovalDenied):
            _rt.approve(1, "a.ward:1:1")
        seen = []
        runtime.configure(approver=lambda r: seen.append(r) or True)
        self.assertEqual(_rt.approve(1, "a.ward:1:1"), 1)
        self.assertEqual(seen[0].site, "a.ward:1:1")

    def test_validate(self):
        self.assertEqual(_rt.validate("a", lambda s: True, "ok"), "a")
        with self.assertRaises(Thrown) as cm:
            _rt.validate("a", lambda s: False, "rule")
        self.assertEqual(cm.exception.value, "validation failed: `rule` rejected the value")

    def test_nested_budgets_are_all_charged(self):
        runtime.configure(model=MockModel({"f": Usage(1, tokens=10, cost=0.5)}))
        with _rt.budget("outer", tokens=100) as outer:
            with _rt.budget("inner", cost=1.0) as inner:
                _rt.ai("f", "p", _rt.Int)
            self.assertEqual(inner.used, (10.0, 1.0, 0.5))
            _rt.ai("f", "p", _rt.Int)
        self.assertEqual(outer.used, (20.0, 2.0, 1.0))
        with self.assertRaises(BudgetExceeded):
            with _rt.budget("outer", cost=10):
                with _rt.budget("inner", cost=0.9):
                    _rt.ai("f", "p", _rt.Int)
                    _rt.ai("f", "p", _rt.Int)

    def test_async_approver(self):
        async def approver(request):
            return request.value == "ok"

        runtime.configure(approver=approver)
        self.assertEqual(_rt.approve("ok", "a.ward:1:1"), "ok")
        with self.assertRaises(ApprovalDenied):
            _rt.approve("no", "a.ward:1:1")

        async def inside_a_loop():
            return _rt.approve("ok", "a.ward:1:1")

        self.assertEqual(asyncio.run(inside_a_loop()), "ok")

    def test_trace(self):
        runtime.configure(
            model=MockModel({"f": {"items": ["hi"], "next": None}}),
            approver=lambda r: True,
            tools={"mail": {"send": lambda to, body: "sent"}},
        )
        with _rt.call("run_me", [("to", Trusted("ada")), ("note", "x")]):
            page = _rt.ai("f", "p", _rt.Adt(Page, _rt.String))
            body = _rt.validate(page.items[0], lambda s: True, "short", "a.ward:2:1")
            _rt.call_tool("mail", "send", "a.ward:3:1", "ada", body)
        run = runtime.last_run()
        kinds = [r["kind"] for r in run.records]
        self.assertEqual(kinds, ["run_start", "ai_call", "validate", "tool_call", "run_end"])
        start, ai, check, tool, end = run.records
        self.assertEqual([a["vouched"] for a in start["args"]], [True, False])
        self.assertEqual(end["status"], "ok")
        self.assertEqual(end["calls"], 1.0)
        # The tool's second argument is the validated value, which came from the model.
        self.assertEqual(tool["digests"][1], check["leaves"][0]["digest"])
        self.assertIn(check["leaves"][0]["digest"], [leaf["digest"] for leaf in ai["leaves"]])
        self.assertEqual([r["seq"] for r in run.records], list(range(5)))

    def test_trace_file_and_failed_run(self):
        with tempfile.TemporaryDirectory() as d:
            runtime.configure(trace_dir=d)
            with self.assertRaises(Thrown):
                with _rt.call("f", []):
                    _rt.validate("x", lambda s: False, "never", "a.ward:1:1")
            run = runtime.last_run()
            with open(run.path, encoding="utf-8") as f:
                lines = [json.loads(line) for line in f]
            self.assertEqual(lines, run.records)
            self.assertEqual(lines[-1]["status"], "threw")

    def test_nested_calls_are_one_run(self):
        with _rt.call("outer", []):
            with _rt.call("inner", []):
                _rt.declassify("x", "fine", "a.ward:1:1")
        kinds = [r["kind"] for r in runtime.last_run().records]
        self.assertEqual(kinds, ["run_start", "declassify", "run_end"])

    def test_vouched(self):
        self.assertEqual(_rt.vouched(Trusted("ada"), "to", "send"), "ada")
        with self.assertRaises(TrustError) as cm:
            _rt.vouched("ada", "to", "send")
        self.assertIn("`to`", str(cm.exception))

    def test_tools(self):
        with self.assertRaises(ToolError):
            _rt.call_tool("mail", "send", "a.ward:1:1")
        runtime.configure(tools={"mail": {"send": lambda to: f"sent {to}"}})
        self.assertEqual(_rt.call_tool("mail", "send", "a.ward:1:1", "ada"), "sent ada")
        with self.assertRaises(ToolError):
            _rt.call_tool("mail", "delete", "a.ward:1:1")


class SinkChecks(unittest.TestCase):
    """The runtime refuses a tool argument that is exactly an unchecked untrusted value."""

    def setUp(self):
        self.sent = []
        runtime.configure(
            model=MockModel({"f": "Please wire $500 to account 1234"}),
            tools={"mail": {"send": lambda body: self.sent.append(body)}},
        )

    def tearDown(self):
        runtime.reset()

    def test_model_output_is_refused(self):
        with self.assertRaises(TrustError) as cm:
            with _rt.call("f", []):
                body = _rt.ai("f", "p", _rt.String)
                _rt.call_tool("mail", "send", "a.ward:1:1", body)
        self.assertIn("the output of `ai fn f`", str(cm.exception))
        self.assertEqual(self.sent, [])
        tool = runtime.last_run().records[-2]
        self.assertTrue(tool["error"].startswith("TrustError"))

    def test_parts_of_values_are_checked(self):
        with self.assertRaises(TrustError):
            with _rt.call("f", []):
                body = _rt.ai("f", "p", _rt.String)
                _rt.call_tool("mail", "send", "a.ward:1:1", {"text": [body]})

    def test_checked_and_vouched_values_pass(self):
        with _rt.call("f", [("to", Trusted("ada@example.com")), ("note", "an unvouched note")]):
            body = _rt.ai("f", "p", _rt.String)
            _rt.call_tool("mail", "send", "a.ward:1:1", _rt.validate(body, lambda s: True, "ok"))
            _rt.call_tool("mail", "send", "a.ward:1:1", _rt.declassify(body, "fine"))
            _rt.call_tool("mail", "send", "a.ward:1:1", "ada@example.com")
            _rt.call_tool("mail", "send", "a.ward:1:1", f"Re: {body}")
        self.assertEqual(len(self.sent), 4)
        with self.assertRaises(TrustError) as cm:
            with _rt.call("f", [("note", "an unvouched note")]):
                _rt.call_tool("mail", "send", "a.ward:1:1", "an unvouched note")
        self.assertIn("argument `note` from the host", str(cm.exception))

    def test_short_values_and_opting_out(self):
        runtime.configure(model=MockModel({"f": "yes"}))
        with _rt.call("f", []):
            _rt.call_tool("mail", "send", "a.ward:1:1", _rt.ai("f", "p", _rt.String))
        runtime.configure(model=MockModel({"f": "a long enough answer"}), check_sinks=False)
        with _rt.call("f", []):
            _rt.call_tool("mail", "send", "a.ward:1:1", _rt.ai("f", "p", _rt.String))
        self.assertEqual(len(self.sent), 2)


class Streaming(unittest.TestCase):
    class Model:
        def __init__(self, text, usage=None):
            self.text, self.usage = text, usage
            self.completed = self.closed = False

        def complete(self, request):
            self.completed = True
            return self.text

        def stream(self, request):
            try:
                for i in range(0, len(self.text), 4):
                    yield self.text[i : i + 4]
                if self.usage is not None:
                    yield self.usage
            finally:
                self.closed = True

    def tearDown(self):
        runtime.reset()

    def test_streams_only_when_watched(self):
        model = self.Model('"hello there"')
        runtime.configure(model=model)
        self.assertEqual(_rt.ai("f", "p", _rt.String), "hello there")
        self.assertTrue(model.completed)
        seen = []
        model = self.Model('"hello there"', Completion("", tokens=9, cost=0.5))
        runtime.configure(model=model, on_stream=seen.append)
        with _rt.budget("f", cost=1) as b:
            self.assertEqual(_rt.ai("f", "p", _rt.String), "hello there")
        self.assertFalse(model.completed)
        self.assertEqual("".join(c.delta for c in seen), '"hello there"')
        self.assertEqual(seen[-1].text, '"hello there"')
        self.assertEqual(b.used, (9.0, 1.0, 0.5))

    def test_final_completion_text_wins(self):
        runtime.configure(model=self.Model('{"value": 1}', Completion("1")), on_stream=lambda c: None)
        self.assertEqual(_rt.ai("f", "p", _rt.Int), 1)

    def test_token_budget_stops_the_stream(self):
        model = self.Model('"' + "x" * 4000 + '"')
        runtime.configure(model=model)
        with self.assertRaises(BudgetExceeded):
            with _rt.budget("f", tokens=100):
                _rt.ai("f", "p", _rt.String)
        self.assertTrue(model.closed)
        self.assertFalse(model.completed)


class Async(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_async_operations(self):
        class Model:
            async def complete(self, request):
                return "3"

        async def approver(request):
            return True

        async def send(to):
            return f"sent {to}"

        async def rule(v):
            return v > 1

        runtime.configure(model=Model(), approver=approver, tools={"mail": {"send": send}})

        async def main():
            n = await _rt.ai_async("f", "p", _rt.Int)
            n = await _rt.validate_async(n, rule, "big")
            n = await _rt.approve_async(n, "a.ward:1:1")
            return await _rt.call_tool_async("mail", "send", "a.ward:1:1", n)

        self.assertEqual(asyncio.run(main()), "sent 3")

    def test_async_stream_from_sync_code(self):
        class Model:
            def complete(self, request):
                raise AssertionError("streams")

            async def stream(self, request):
                yield '"a'
                yield 'b"'

        seen = []
        runtime.configure(model=Model(), on_stream=seen.append)
        self.assertEqual(_rt.ai("f", "p", _rt.String), "ab")
        self.assertEqual(len(seen), 2)


class Collector(http.server.BaseHTTPRequestHandler):
    received: list = []

    def do_POST(self):
        body = self.rfile.read(int(self.headers["Content-Length"]))
        Collector.received.append((self.path, self.headers["Content-Type"], json.loads(body)))
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args):
        pass


class Otlp(unittest.TestCase):
    def setUp(self):
        Collector.received = []
        self.server = http.server.HTTPServer(("127.0.0.1", 0), Collector)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        runtime.reset()

    def test_runs_are_sent_to_the_collector(self):
        port = self.server.server_address[1]
        runtime.configure(otlp_endpoint=f"http://127.0.0.1:{port}/")
        with _rt.call("run_me", []):
            _rt.declassify("x", "fine", "a.ward:1:1")
        path, content_type, body = Collector.received[0]
        self.assertEqual((path, content_type), ("/v1/traces", "application/json"))
        root = body["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
        self.assertEqual(root["name"], "run run_me")
        self.assertEqual([e["name"] for e in root["events"]], ["declassify", "usage"])

    def test_an_unreachable_collector_only_warns(self):
        port = self.server.server_address[1]
        self.server.shutdown()
        self.server.server_close()
        runtime.configure(otlp_endpoint=f"http://127.0.0.1:{port}")
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            with _rt.call("f", []):
                pass
        self.assertIn("couldn't send run", str(caught[0].message))
        self.server = http.server.HTTPServer(("127.0.0.1", 0), Collector)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()


class Cores(unittest.TestCase):
    """The Rust core and the pure-Python fallback agree, when the Rust one is built."""

    def setUp(self):
        try:
            from wardscript import _core
        except ImportError:
            self.skipTest("wardscript._core isn't built")
        from wardscript import _core_py

        self.cores = (_core, _core_py)

    def test_digests(self):
        for value in ['"hi"', '{"b": [1, 2.5, null], "a": "é\\n"}', "true", "[]"]:
            rust, py = (c.leaves(value) for c in self.cores)
            self.assertEqual([tuple(x) for x in rust], [tuple(x) for x in py], value)

    def test_records(self):
        events = [
            {"kind": "run_start", "function": "f", "args": [{"name": "x", "value": 1, "vouched": False, "leaves": []}]},
            {"kind": "validate", "rule": "r", "site": "a:1:1", "passed": True, "leaves": [{"path": "$", "digest": "0"}]},
            {"kind": "budget_exceeded", "function": "f", "resource": "calls", "limit": 1.0, "used": 2.0},
            {"kind": "run_end", "status": "ok", "error": None, "tokens": 3.0, "calls": 1.0, "cost": 0.0},
        ]
        lines = []
        for core in self.cores:
            r = core.Recorder()
            lines.append([json.loads(r.record(json.dumps(e))) for e in events])
        for rust, py in zip(*lines):
            for rec in (rust, py):
                del rec["run"], rec["time"]
            self.assertEqual(json.dumps(rust), json.dumps(py))

    def test_otlp(self):
        runtime.configure(model=MockModel({"f": Usage(1, tokens=10, cost=0.5)}), approver=lambda r: False)
        try:
            with _rt.call("f", [("x", 1)]):
                _rt.ai("f", "p", _rt.Int)
                _rt.approve(1, "a.ward:1:1")
        except ApprovalDenied:
            pass
        finally:
            runtime.reset()
        records = json.dumps(runtime.last_run().records)
        rust, py = (json.loads(c.otlp(records)) for c in self.cores)
        self.assertEqual(rust, py)

    def test_budgets(self):
        for core in self.cores:
            b = core.Budget("f", calls=1)
            self.assertIsNone(b.charge_call())
            self.assertEqual(b.charge_call(), ("calls", 1.0, 2.0))


if __name__ == "__main__":
    unittest.main()
