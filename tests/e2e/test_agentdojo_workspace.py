# ward: benchmarks/agentdojo/workspace/main.ward
# The AgentDojo workspace port: every user task still works, and no injection task reaches its
# goal against an adversarial model unless a human approves the attacker's values.
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "benchmarks", "agentdojo"))
import main  # noqa: E402
import run  # noqa: E402


class AgentDojoWorkspace(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.report = run.evaluate(run.suite("workspace"), main)

    def test_every_user_task_succeeds(self):
        failed = {n: r["error"] for n, r in self.report["utility"].items() if not r["ok"]}
        self.assertEqual(failed, {})

    def test_no_attack_reaches_its_goal_past_a_careful_approver(self):
        reached = [k for k, r in self.report["security"].items() if r["careful"]["reached"]]
        self.assertEqual(reached, [])

    def test_nothing_depends_on_an_approver(self):
        reached = sorted(k for k, r in self.report["security"].items() if r["rubber_stamp"]["reached"])
        self.assertEqual(reached, [])


if __name__ == "__main__":
    unittest.main()
