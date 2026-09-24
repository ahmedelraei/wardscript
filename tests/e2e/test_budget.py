# ward: tests/e2e/budget.ward
import time
import unittest

import budget
from wardscript import BudgetExceeded, Trusted, runtime
from wardscript.mock import MockModel, Raw, Seq, Usage


class Clock:
    def sleep(self, seconds):
        time.sleep(seconds)


class Budgets(unittest.TestCase):
    def setUp(self):
        runtime.configure(tools={"clock": Clock()})

    def tearDown(self):
        runtime.reset()

    def test_within_budget(self):
        runtime.configure(model=MockModel({"ask": "ok"}))
        self.assertEqual(budget.chat("hi", 3), "ok")

    def test_calls(self):
        model = MockModel({"ask": "ok"})
        runtime.configure(model=model)
        with self.assertRaises(BudgetExceeded) as cm:
            budget.chat("hi", 4)
        self.assertEqual((cm.exception.function, cm.exception.resource), ("chat", "calls"))
        # The fourth call is refused before it reaches the model.
        self.assertEqual(len(model.calls), 3)

    def test_tokens(self):
        runtime.configure(model=MockModel({"ask": Usage("ok", tokens=600)}))
        with self.assertRaises(BudgetExceeded) as cm:
            budget.chat("hi", 2)
        self.assertEqual(cm.exception.resource, "tokens")
        self.assertEqual(cm.exception.used, 1200)

    def test_cost(self):
        runtime.configure(model=MockModel({"ask": Usage("ok", tokens=1, cost=0.03)}))
        with self.assertRaises(BudgetExceeded) as cm:
            budget.chat("hi", 2)
        self.assertEqual(cm.exception.resource, "cost")
        self.assertIn("$0.06 of $0.05", str(cm.exception))

    def test_retries_count_as_calls(self):
        runtime.configure(model=MockModel({"strict": Seq(Raw("not json"), "ok")}))
        with self.assertRaises(BudgetExceeded) as cm:
            budget.once("hi")
        self.assertEqual((cm.exception.function, cm.exception.resource), ("strict", "calls"))

        runtime.configure(model=MockModel({"strict": "ok"}))
        self.assertEqual(budget.once("hi"), "ok")

    def test_time(self):
        self.assertEqual(budget.slow(Trusted(0.0)), "done")
        with self.assertRaises(BudgetExceeded) as cm:
            budget.slow(Trusted(0.1))
        self.assertEqual(cm.exception.resource, "time")

    def test_budgets_end_with_their_function(self):
        runtime.configure(model=MockModel({"ask": "ok"}))
        for _ in range(3):
            self.assertEqual(budget.chat("hi", 3), "ok")


if __name__ == "__main__":
    unittest.main()
