# ward: tests/e2e/fallback.ward
# M8 acceptance: a retry after a rate limit, a fallback after a failing primary, and
# budgets charged for every attempt, each one in the trace.
import unittest

import fallback
from wardscript import BudgetExceeded, ModelUnavailable, RateLimited, runtime
from wardscript.mock import MockModel, Seq, Usage


def attempts():
    return [(r["model"], r["error"]) for r in runtime.last_run().records if r["kind"] == "ai_call"]


class Fallback(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_retry_after_a_rate_limit(self):
        runtime.configure(models={"fast": MockModel({"summarize": Seq(RateLimited("429"), "short")})})
        self.assertEqual(fallback.digest("a long text"), "short")
        self.assertEqual(attempts(), [("fast", "RateLimited: 429"), ("fast", None)])

    def test_fallback_after_a_failing_primary(self):
        runtime.configure(
            models={
                "fast": MockModel({"summarize": Seq(ModelUnavailable("503"), ModelUnavailable("503"))}),
                "smart": MockModel({"summarize": Usage("from smart", tokens=50)}),
            }
        )
        self.assertEqual(fallback.digest("a long text"), "from smart")
        self.assertEqual(
            attempts(),
            [("fast", "ModelUnavailable: 503"), ("fast", "ModelUnavailable: 503"), ("smart", None)],
        )
        end = runtime.last_run().records[-1]
        self.assertEqual((end["calls"], end["tokens"]), (3.0, 50.0))

    def test_budget_counts_failed_attempts(self):
        runtime.configure(
            models={
                "fast": MockModel({"summarize": Seq(RateLimited("429"), RateLimited("429"))}),
                "smart": MockModel({"summarize": Seq(RateLimited("429"), "late")}),
            }
        )
        # Two failed fast calls and a rate-limited smart one use the budget of 3; the
        # smart model's retry would be the 4th call, so it's never sent.
        with self.assertRaises(BudgetExceeded):
            fallback.digest("a long text")
        self.assertEqual(len(attempts()), 3)


if __name__ == "__main__":
    unittest.main()
