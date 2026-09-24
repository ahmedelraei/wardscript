# Runs examples/support.wardscript once, writing its trace to argv[1]; the email asks for
# a refund, so the reply goes to a human before it's sent. See crates/ward_cli/tests/trace.rs.
import sys

import support
from wardscript import Trusted, runtime
from wardscript.mock import MockModel, Usage


class Mail:
    def send(self, to, subject, body):
        return "queued"


runtime.configure(
    model=MockModel(
        {
            "triage": Usage(
                {"customer": "Ada", "summary": "refund for a late parcel", "priority": "Normal", "refund_requested": True},
                tokens=420,
                cost=0.002,
            ),
            "draft_reply": Usage({"subject": "Your refund", "body": "It's on its way."}, tokens=380, cost=0.003),
        }
    ),
    approver=lambda request: True,
    tools={"gmail": Mail()},
    trace_dir=sys.argv[1],
)
print(support.handle("Where is my parcel? I want a refund.", Trusted("ada@example.com")))
