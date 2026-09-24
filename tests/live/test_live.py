# Runs the triage and support examples against a real model. Only with WARD_LIVE=1 and
# an API key; the harness is crates/ward_cli/tests/live.rs, which builds both examples
# and passes the provider (WARD_LIVE_MODEL, default `anthropic`).
import os
import unittest

import support
import triage
from wardscript import Trusted, runtime
from wardscript.providers import load


class Mail:
    def __init__(self):
        self.sent = []

    def send(self, to, subject, body):
        self.sent.append((to, subject, body))


class Live(unittest.TestCase):
    def setUp(self):
        self.model = load(os.environ.get("WARD_LIVE_MODEL", "anthropic"))
        self.mail = Mail()
        runtime.configure(model=self.model, tools={"gmail": self.mail}, approver=lambda r: True)

    def tearDown(self):
        runtime.reset()

    def test_triage_returns_a_ticket(self):
        ticket = triage.triage("Hi, I was charged twice for order 1042. Please refund one. - Ada")
        self.assertIsInstance(ticket, triage.Ticket)
        self.assertIsInstance(ticket.priority, triage.Priority)
        run = runtime.last_run()
        end = run.records[-1]
        self.assertEqual(end["status"], "ok")
        # Usage the provider reported, not the length-based estimate.
        ai = [r for r in run.records if r["kind"] == "ai_call"]
        self.assertTrue(all(r["tokens"] > 0 for r in ai))
        self.assertEqual(end["tokens"], sum(r["tokens"] for r in ai))

    def test_support_sends_a_checked_reply(self):
        result = support.handle(
            "Hello, my parcel (order 77) hasn't arrived after two weeks. Where is it? - Ada",
            Trusted("ada@example.com"),
        )
        self.assertTrue(result.startswith("sent"), result)
        self.assertEqual(len(self.mail.sent), 1)
        to, subject, body = self.mail.sent[0]
        self.assertEqual(to, "ada@example.com")
        self.assertNotIn("\n", subject)


if __name__ == "__main__":
    unittest.main()
