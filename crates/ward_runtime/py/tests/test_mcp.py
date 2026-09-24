import os
import sys
import tempfile
import unittest

from wardscript import Thrown, ToolError, TrustError, _rt, mcp, runtime
from wardscript.mock import MockModel

SERVER = os.path.join(os.path.dirname(__file__), "..", "..", "..", "..", "examples", "inbox", "gmail_server.py")


class Client(unittest.TestCase):
    """The stdio client against the example's Gmail-like server."""

    def setUp(self):
        self.server = mcp.Server(sys.executable, [SERVER], name="gmail")

    def tearDown(self):
        self.server.close()

    def test_lists_and_calls_tools(self):
        names = [t["name"] for t in self.server.list_tools()]
        self.assertEqual(names, ["list_messages", "read_message", "label_message", "send_email"])
        self.assertEqual(self.server.call_tool("list_messages", {"query": "x", "limit": 2}), "m1\nm2")

    def test_errors_are_thrown(self):
        with self.assertRaises(Thrown) as cm:
            self.server.call_tool("read_message", {"id": "nope"})
        self.assertEqual(cm.exception.value, "no message nope")
        with self.assertRaises(ToolError):
            self.server.call_tool("unknown", {})

    def test_a_missing_command(self):
        with self.assertRaises(ToolError):
            mcp.Server("/no/such/server").list_tools()

    def test_load_config(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "mcp.json")
            with open(path, "w", encoding="utf-8") as f:
                f.write('{"mcpServers": {"a": {"command": "x", "args": ["y"]}, "b": {"url": "http://z"}}}')
            servers = mcp.load_config(path)
        self.assertEqual(list(servers), ["a"])
        self.assertEqual(servers["a"].command, ["x", "y"])


class TypedToolCalls(unittest.TestCase):
    """What generated code passes for a tool with a schema in `ward.lock`."""

    def setUp(self):
        self.outbox = tempfile.NamedTemporaryFile(delete=False)
        self.outbox.close()
        server = mcp.Server(sys.executable, [SERVER], env={"GMAIL_OUTBOX": self.outbox.name})
        self.server = server
        runtime.configure(tools={"gmail": server}, model=MockModel({"f": "a long enough answer"}))

    def tearDown(self):
        runtime.reset()
        self.server.close()
        os.unlink(self.outbox.name)

    def send(self, *args, sinks=(True, True, True, True)):
        return _rt.call_tool(
            "gmail", "send_email", "a.ward:1:1", *args,
            names=("to", "subject", "body", "cc"), sinks=sinks, returns=_rt.String,
        )

    def test_named_arguments_and_typed_result(self):
        self.assertEqual(self.send("a@example.com", "s", "b"), "sent")
        with open(self.outbox.name, encoding="utf-8") as f:
            self.assertIn('"to": "a@example.com"', f.read())
        # `mcp_name` when the server's name isn't a Wardscript name.
        ids = _rt.call_tool(
            "gmail", "list", "a.ward:1:1", "q", None,
            mcp_name="list_messages", names=("query", "limit"), sinks=(False, False), returns=_rt.String,
        )
        self.assertEqual(ids, "m1\nm2\nm3")

    def test_result_must_match_the_schema(self):
        with self.assertRaises(ToolError):
            _rt.call_tool(
                "gmail", "list_messages", "a.ward:1:1", "q",
                names=("query", "limit"), sinks=(False, False), returns=_rt.Int,
            )

    def test_only_sinks_are_checked(self):
        with _rt.call("f", []):
            answer = _rt.ai("f", "p", _rt.String)
            self.send("a@example.com", "s", answer, sinks=(True, True, False, True))
            with self.assertRaises(TrustError):
                self.send("a@example.com", "s", answer)

    def test_plain_functions_still_get_positional_arguments(self):
        seen = []
        runtime.configure(tools={"gmail": {"send_email": lambda *a: seen.append(a) or "ok"}})
        self.assertEqual(self.send("a@example.com", "s", "b"), "ok")
        self.assertEqual(seen, [("a@example.com", "s", "b")])


if __name__ == "__main__":
    unittest.main()
