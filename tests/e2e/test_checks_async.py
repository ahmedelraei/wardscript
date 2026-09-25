# ward: tests/e2e/checks.ward --async
# Checks in async code: the check function and its model judge are awaited.
import asyncio
import unittest

import checks
from wardscript import runtime
from wardscript.mock import MockModel, Seq

GOOD = {"subject": "Hello", "body": "Hi Ada, thanks for writing."}


class AsyncChecks(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_checks_are_awaited(self):
        runtime.configure(model=MockModel({"draft": Seq(dict(GOOD, body="Hi Ada, see http://x"), GOOD), "is_polite": True}))
        self.assertEqual(asyncio.run(checks.reply("Ada")), checks.Reply(**GOOD))
        errors = [r["error"] for r in runtime.last_run().records if r["kind"] == "ai_call" and r["function"] == "draft"]
        self.assertEqual(errors, ["the answer failed a check: no links", None])


if __name__ == "__main__":
    unittest.main()
