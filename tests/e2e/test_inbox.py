# ward: examples/inbox/main.ward
# M7 acceptance: tool types come from `ward.lock`, and the calls reach a real MCP server
# (the example's Gmail-like one) with named arguments.
import json
import os
import sys
import tempfile
import unittest

import main
from wardscript import Thrown, mcp, runtime
from wardscript.mock import MockModel

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
EXAMPLE = os.path.join(ROOT, "examples", "inbox")


class Inbox(unittest.TestCase):
    def setUp(self):
        self.outbox = tempfile.NamedTemporaryFile(suffix=".jsonl", delete=False)
        self.outbox.close()
        os.environ["GMAIL_OUTBOX"] = self.outbox.name
        self.servers = mcp.load_config(os.path.join(EXAMPLE, "mcp.json"))
        for s in self.servers.values():
            s.command[0] = sys.executable  # The interpreter running the tests.

    def tearDown(self):
        runtime.reset()
        for s in self.servers.values():
            s.close()
        del os.environ["GMAIL_OUTBOX"]
        os.unlink(self.outbox.name)

    def sent(self):
        with open(self.outbox.name, encoding="utf-8") as f:
            return [json.loads(line) for line in f]

    def test_triage_labels_and_reports_a_count(self):
        answers = iter(["Billing", "Support", "Spam"])
        runtime.configure(
            tools=self.servers,
            model=MockModel({"classify": lambda request: next(answers)}),
        )
        self.assertEqual(main.triage_inbox(main_owner()), 3)
        events = self.sent()
        self.assertEqual([e.get("label") for e in events[:3]], ["billing", "support", "spam"])
        self.assertEqual(events[3], {"to": "owner@example.com", "subject": "Inbox triage", "body": "3 messages labeled"})
        # The injection in m3 went nowhere: only the owner got mail.
        self.assertEqual([e["to"] for e in events if "to" in e], ["owner@example.com"])
        calls = [r for r in runtime.last_run().records if r["kind"] == "tool_call"]
        self.assertEqual(len(calls), 1 + 3 * 2 + 1)

    def test_tool_errors_are_thrown(self):
        class Broken(mcp.Server):
            def call_tool(self, name, arguments):
                raise Thrown("mailbox unavailable")

        runtime.configure(tools={"gmail": Broken("x")}, model=MockModel({}))
        with self.assertRaises(Thrown) as cm:
            main.triage_inbox(main_owner())
        self.assertEqual(cm.exception.value, "mailbox unavailable")


def main_owner():
    from wardscript import Trusted

    return Trusted("owner@example.com")


if __name__ == "__main__":
    unittest.main()
