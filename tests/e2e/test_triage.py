# ward: examples/triage.wardscript
# M3 acceptance: the mock model's answer comes back as a correctly typed `Ticket`.
import unittest

import triage
from wardscript import AiOutputError, NoModelError, runtime
from wardscript.mock import MockModel, Raw, Seq

ANSWER = {
    "customer": "Ada",
    "summary": "Checkout crashes",
    "priority": "Urgent",
    "category": {"Bug": ["checkout"]},
    "tags": ["crash", "web"],
    "order_id": 1042,
}


class Triage(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_returns_a_typed_ticket(self):
        model = MockModel({"triage": ANSWER})
        runtime.configure(model=model)
        ticket = triage.triage("My checkout crashed on order 1042")

        self.assertIsInstance(ticket, triage.Ticket)
        self.assertEqual(ticket.customer, "Ada")
        self.assertIs(ticket.priority, triage.Priority.Urgent)
        self.assertEqual(ticket.category, triage.Category.Bug("checkout"))
        self.assertEqual(ticket.tags, ["crash", "web"])
        self.assertEqual(ticket.order_id, 1042)

        [request] = model.calls
        self.assertEqual(request.function, "triage")
        self.assertIn("Email:\nMy checkout crashed on order 1042", request.prompt)
        ticket_schema = request.schema["$defs"]["Ticket"]
        self.assertEqual(ticket_schema["required"], list(ANSWER))
        self.assertEqual(request.schema["$defs"]["Priority"]["enum"], ["Low", "Normal", "Urgent"])

    def test_generated_code_uses_the_ticket(self):
        runtime.configure(model=MockModel({"triage": {**ANSWER, "order_id": None, "priority": "Low"}}))
        self.assertEqual(triage.route("..."), "engineering/checkout: Checkout crashes")

    def test_invalid_answers_are_retried(self):
        model = MockModel({"triage": Seq(Raw("not json"), {**ANSWER, "priority": "Soon"}, ANSWER)})
        runtime.configure(model=model)
        self.assertEqual(triage.triage("...").customer, "Ada")
        self.assertEqual([r.attempt for r in model.calls], [0, 1, 2])
        self.assertIn("not valid JSON", model.calls[1].errors[0])
        self.assertIn("$.priority", model.calls[2].errors[1])

    def test_gives_up_after_the_retries(self):
        runtime.configure(model=MockModel({"triage": {**ANSWER, "tags": "crash"}}), retries=1)
        with self.assertRaises(AiOutputError) as cm:
            triage.triage("...")
        self.assertEqual(len(cm.exception.errors), 2)
        self.assertIn("$.tags: expected an array", cm.exception.errors[0])

    def test_needs_a_model(self):
        with self.assertRaises(NoModelError):
            triage.triage("...")


if __name__ == "__main__":
    unittest.main()
