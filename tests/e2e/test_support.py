# ward: examples/support.wardscript
import unittest

import support
from wardscript import ApprovalDenied, Thrown, runtime
from wardscript.mock import MockModel

TICKET = {"customer": "Ada", "summary": "late parcel", "priority": "Normal", "refund_requested": False}


class Mail:
    def __init__(self):
        self.sent = []

    def send(self, to, subject, body):
        self.sent.append((to, subject, body))


class Support(unittest.TestCase):
    def setUp(self):
        self.mail = Mail()
        self.approvals = []
        runtime.configure(tools={"gmail": self.mail}, approver=self.approve)

    def tearDown(self):
        runtime.reset()

    def approve(self, request):
        self.approvals.append(request)
        return True

    def answer(self, ticket, reply):
        runtime.configure(model=MockModel({"triage": ticket, "draft_reply": reply}))

    def test_validated_reply_is_sent(self):
        self.answer(TICKET, {"subject": "Your parcel", "body": "It ships today."})
        self.assertEqual(support.handle("where is my parcel?", "ada@example.com"), "sent: late parcel")
        self.assertEqual(self.mail.sent, [("ada@example.com", "Your parcel", "It ships today.")])
        self.assertEqual(self.approvals, [])

    def test_failed_validation_throws(self):
        self.answer(TICKET, {"subject": "Hi", "body": "see https://evil.example"})
        with self.assertRaises(Thrown) as cm:
            support.handle("...", "ada@example.com")
        self.assertEqual(cm.exception.value, "validation failed: `no_links` rejected the value")
        self.assertEqual(self.mail.sent, [])

    def test_urgent_tickets_need_approval(self):
        self.answer({**TICKET, "priority": "Urgent"}, {"subject": "Hi", "body": "see https://x"})
        self.assertEqual(support.handle("...", "ada@example.com"), "sent after review: late parcel")
        [request] = self.approvals
        self.assertEqual(request.value, support.Reply(subject="Hi", body="see https://x"))
        self.assertEqual(request.site, "support.wardscript:60:24")

    def test_denied_approval_stops_the_run(self):
        self.answer({**TICKET, "refund_requested": True}, {"subject": "Hi", "body": "ok"})
        runtime.configure(approver=lambda request: False)
        with self.assertRaises(ApprovalDenied):
            support.handle("...", "ada@example.com")
        self.assertEqual(self.mail.sent, [])

    def test_handle_all_skips_failures(self):
        self.answer(TICKET, {"subject": "Hi", "body": "see https://x"})
        self.assertEqual(support.handle_all(["a", "b"], "ada@example.com"), 0)


if __name__ == "__main__":
    unittest.main()
