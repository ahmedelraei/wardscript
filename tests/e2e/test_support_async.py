# ward: examples/support.wardscript --async
# The support example built with `ward build --async`, run by an asyncio host with async
# model, approver and tools, and a model that streams its answers.
import asyncio
import json
import unittest

import support
from wardscript import BudgetExceeded, Completion, Trusted, runtime

ADA = Trusted("ada@example.com")
TICKET = {"customer": "Ada", "summary": "late parcel", "priority": "Normal", "refund_requested": False}
REFUND = dict(TICKET, refund_requested=True)
REPLY = {"subject": "Your parcel", "body": "It ships today."}


class AsyncModel:
    """Answers after yielding to the event loop, so concurrent runs interleave."""

    def __init__(self, answers):
        self.answers = answers

    async def complete(self, request):
        await asyncio.sleep(0)
        # Free, so `handle`'s cost budget can count it.
        return Completion(json.dumps(self.answers[request.function]), cost=0.0)


class StreamingModel(AsyncModel):
    async def stream(self, request):
        text = json.dumps(self.answers[request.function])
        for i in range(0, len(text), 5):
            await asyncio.sleep(0)
            yield text[i : i + 5]
        yield Completion("", tokens=len(text), cost=0.0)


class Mail:
    def __init__(self):
        self.sent = []

    async def send(self, to, subject, body):
        await asyncio.sleep(0)
        self.sent.append((to, subject, body))


class SupportAsync(unittest.TestCase):
    def setUp(self):
        self.mail = Mail()
        runtime.configure(tools={"gmail": self.mail}, approver=self.approve)

    def tearDown(self):
        runtime.reset()

    async def approve(self, request):
        await asyncio.sleep(0)
        return True

    def test_concurrent_runs_have_their_own_traces(self):
        runtime.configure(model=AsyncModel({"triage": TICKET, "draft_reply": REPLY}))

        async def main():
            return await asyncio.gather(*(support.handle(f"email {i}", ADA) for i in range(3)))

        self.assertEqual(asyncio.run(main()), ["sent: late parcel"] * 3)
        self.assertEqual(len(self.mail.sent), 3)
        kinds = [r["kind"] for r in runtime.last_run().records]
        self.assertEqual(kinds.count("run_start"), 1)
        self.assertEqual(kinds.count("tool_call"), 1)

    def test_async_approver(self):
        runtime.configure(model=AsyncModel({"triage": REFUND, "draft_reply": REPLY}))
        self.assertEqual(asyncio.run(support.handle("refund", ADA)), "sent after review: late parcel")
        self.assertEqual(self.mail.sent, [("ada@example.com", "Your parcel", "It ships today.")])

    def test_streaming(self):
        seen = []
        runtime.configure(
            model=StreamingModel({"triage": TICKET, "draft_reply": REPLY}),
            on_stream=lambda chunk: seen.append(chunk),
        )
        self.assertEqual(asyncio.run(support.handle("hi", ADA)), "sent: late parcel")
        self.assertEqual({c.function for c in seen}, {"triage", "draft_reply"})
        self.assertEqual(json.loads([c for c in seen if c.function == "triage"][-1].text), TICKET)
        ai = [r for r in runtime.last_run().records if r["kind"] == "ai_call"]
        self.assertEqual(ai[0]["tokens"], float(len(json.dumps(TICKET))))

    def test_a_token_budget_stops_a_stream(self):
        long = dict(TICKET, summary="x" * 40_000)
        chunks = []
        runtime.configure(
            model=StreamingModel({"triage": long, "draft_reply": REPLY}),
            on_stream=lambda chunk: chunks.append(chunk),
        )
        with self.assertRaises(BudgetExceeded) as cm:
            asyncio.run(support.handle("hi", ADA))
        self.assertEqual(cm.exception.resource, "tokens")
        # Stopped once the answer went over `triage`'s 2000 tokens, long before its end.
        self.assertEqual(cm.exception.function, "triage")
        self.assertLess(len(chunks[-1].text), 10_000)


if __name__ == "__main__":
    unittest.main()
