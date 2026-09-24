import asyncio
import unittest
from unittest import mock

from wardscript import (
    AiOutputError,
    BudgetExceeded,
    BudgetUnenforceable,
    Completion,
    ModelError,
    ModelUnavailable,
    NoModelError,
    RateLimited,
    _rt,
    runtime,
)
from wardscript.mock import MockModel, Raw, Seq, Usage
from wardscript.providers import model_error


def ai_calls():
    return [r for r in runtime.last_run().records if r["kind"] == "ai_call"]


class Policies(unittest.TestCase):
    """M8: `model {primary, fallback, retries, backoff}` at run time."""

    def setUp(self):
        runtime.configure(backoff=0)

    def tearDown(self):
        runtime.reset()

    def run_ai(self, **policy):
        with _rt.call("f", []):
            return _rt.ai("f", "p", _rt.Int, **policy)

    def test_falls_back_when_the_primary_fails(self):
        runtime.configure(
            models={
                "fast": MockModel({"f": ModelError("bad request", status=400)}),
                "smart": MockModel({"f": 7}),
            }
        )
        self.assertEqual(self.run_ai(models=("fast", "smart")), 7)
        calls = ai_calls()
        self.assertEqual([(c["model"], c["attempt"]) for c in calls], [("fast", 0), ("smart", 1)])
        self.assertEqual(calls[0]["error"], "ModelError: bad request")
        self.assertIsNone(calls[1]["error"])

    def test_retries_a_rate_limit_with_backoff(self):
        runtime.configure(models={"fast": MockModel({"f": Seq(RateLimited("slow"), RateLimited("slow"), 3)})})
        with mock.patch("time.sleep") as sleep:
            self.assertEqual(self.run_ai(models=("fast",), retries=2, backoff=0.5), 3)
        self.assertEqual([c.args[0] for c in sleep.call_args_list], [0.5, 1.0])
        self.assertEqual(len(ai_calls()), 3)

    def test_retries_run_out_then_fallback(self):
        runtime.configure(
            models={
                "fast": MockModel({"f": Seq(ModelUnavailable("down"), ModelUnavailable("down"))}),
                "smart": MockModel({"f": 1}),
            }
        )
        self.assertEqual(self.run_ai(models=("fast", "smart"), retries=1), 1)
        self.assertEqual([c["model"] for c in ai_calls()], ["fast", "fast", "smart"])

    def test_invalid_answers_fall_back_too(self):
        runtime.configure(
            retries=1,
            models={"fast": MockModel({"f": Seq(Raw("no"), Raw("still no"))}), "smart": MockModel({"f": 2})},
        )
        self.assertEqual(self.run_ai(models=("fast", "smart")), 2)
        calls = ai_calls()
        self.assertEqual([c["model"] for c in calls], ["fast", "fast", "smart"])
        self.assertEqual([c["attempt"] for c in calls], [0, 1, 2])

    def test_the_last_error_is_raised(self):
        runtime.configure(models={"a": MockModel({"f": Raw("x")}), "b": MockModel({"f": ModelError("no")})})
        with self.assertRaises(ModelError):
            self.run_ai(models=("a", "b"), retries=0)
        runtime.configure(retries=0, models={"a": MockModel({"f": ModelError("no")}), "b": MockModel({"f": Raw("x")})})
        with self.assertRaises(AiOutputError):
            self.run_ai(models=("a", "b"))

    def test_default_model_first(self):
        runtime.configure(model=MockModel({"f": ModelError("no")}), models={"smart": MockModel({"f": 4})})
        self.assertEqual(self.run_ai(models=(None, "smart")), 4)
        self.assertEqual([c["model"] for c in ai_calls()], [None, "smart"])

    def test_unknown_alias(self):
        runtime.configure(models={"fast": MockModel({"f": 1})})
        with self.assertRaises(NoModelError) as cm:
            self.run_ai(models=("fastest",))
        self.assertIn("`fastest`", str(cm.exception))
        self.assertIn("`fast`", str(cm.exception))

    def test_other_exceptions_are_not_retried(self):
        class Broken:
            def complete(self, request):
                raise KeyError("bug")

        runtime.configure(models={"a": Broken(), "b": MockModel({"f": 1})})
        with self.assertRaises(KeyError):
            self.run_ai(models=("a", "b"))

    def test_every_attempt_is_charged(self):
        runtime.configure(
            models={
                "fast": MockModel({"f": RateLimited("slow")}),
                "smart": MockModel({"f": Usage(5, tokens=40, cost=0.25)}),
            }
        )
        with _rt.budget("f", calls=10, cost=1) as b:
            self.assertEqual(self.run_ai(models=("fast", "smart"), retries=1), 5)
        self.assertEqual(b.used, (40.0, 3.0, 0.25))
        with self.assertRaises(BudgetExceeded):
            with _rt.budget("f", calls=2):
                self.run_ai(models=("fast", "smart"), retries=1)

    def test_prices_are_per_model(self):
        class Unpriced:
            prices = None

            def complete(self, request):
                return Completion("1", 10, None)

        runtime.configure(models={"fast": MockModel({"f": ModelError("no")}), "cheap": Unpriced()})
        with self.assertRaises(BudgetUnenforceable):
            with _rt.budget("f", cost=1):
                self.run_ai(models=("fast", "cheap"))

    def test_async(self):
        runtime.configure(
            models={"fast": MockModel({"f": Seq(RateLimited("slow"), ModelError("no"))}), "smart": MockModel({"f": 9})}
        )

        async def main():
            with _rt.call("f", []):
                return await _rt.ai_async("f", "p", _rt.Int, models=("fast", "smart"), retries=3)

        self.assertEqual(asyncio.run(main()), 9)
        self.assertEqual([c["model"] for c in ai_calls()], ["fast", "fast", "smart"])

    def test_configure_checks_values(self):
        with self.assertRaises(ValueError):
            runtime.configure(model_retries=-1)
        with self.assertRaises(ValueError):
            runtime.configure(backoff=-1)


class ProviderErrors(unittest.TestCase):
    def test_sdk_exceptions_map_by_status_and_kind(self):
        class RateLimitError(Exception):
            status_code = 429

        class APITimeoutError(Exception):
            pass

        class InternalServerError(Exception):
            status_code = 529

        class BadRequestError(Exception):
            status_code = 400

        self.assertIsInstance(model_error(RateLimitError("x")), RateLimited)
        self.assertIsInstance(model_error(APITimeoutError("x")), ModelUnavailable)
        self.assertIsInstance(model_error(InternalServerError("x")), ModelUnavailable)
        e = model_error(BadRequestError("x"))
        self.assertIs(type(e), ModelError)
        self.assertFalse(e.retryable)
        self.assertEqual(e.status, 400)
        self.assertIsNone(model_error(ValueError("x")))


if __name__ == "__main__":
    unittest.main()
