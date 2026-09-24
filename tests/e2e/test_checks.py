# ward: tests/e2e/checks.ward
# M9 acceptance: a failed refinement or check causes a retry with the reason, then
# success or `AiOutputError`; a model judge can be a check.
import unittest

import checks
from wardscript import AiOutputError, runtime
from wardscript.mock import MockModel, Seq

GOOD = {"subject": "Hello", "body": "Hi Ada, thanks for writing."}


def attempts():
    return [(r["function"], r["error"]) for r in runtime.last_run().records if r["kind"] == "ai_call"]


class Checks(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_a_failed_check_is_retried_with_its_reason(self):
        model = MockModel({"draft": Seq(dict(GOOD, body="Hi there"), GOOD), "is_polite": True})
        runtime.configure(model=model)
        self.assertEqual(checks.reply("Ada"), checks.Reply(**GOOD))
        self.assertEqual(
            attempts(),
            [
                ("draft", "the answer failed a check: greet the customer by name"),
                ("is_polite", None),
                ("draft", None),
            ],
        )
        retry = [c for c in model.calls if c.function == "draft"][1]
        self.assertIn("greet the customer by name", retry.instructions())

    def test_a_refinement_is_checked_while_decoding(self):
        runtime.configure(model=MockModel({"draft": Seq(dict(GOOD, subject="x" * 40), GOOD), "is_polite": True}))
        checks.reply("Ada")
        self.assertEqual(attempts()[0], ("draft", "$.subject: doesn't satisfy `it.len() <= 30`"))

    def test_a_model_judge_fails_the_answer(self):
        runtime.configure(retries=1, model=MockModel({"draft": GOOD, "is_polite": False}))
        with self.assertRaises(AiOutputError) as cm:
            checks.reply("Ada")
        self.assertEqual(cm.exception.errors, ["the answer failed a check: be polite"] * 2)

    def test_the_schema_carries_the_refinements(self):
        from wardscript import _rt, json_schema

        reply = json_schema(_rt.Adt(checks.Reply))["$defs"]["Reply"]["properties"]
        self.assertEqual(reply["subject"]["maxLength"], 30)
        self.assertEqual(reply["body"]["minLength"], 1)


if __name__ == "__main__":
    unittest.main()
