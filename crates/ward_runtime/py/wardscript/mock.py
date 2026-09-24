"""A deterministic model for tests: scripted answers per `ai fn`."""

from __future__ import annotations

import json
from typing import Any, Mapping

from .errors import WardError
from .model import AiRequest, Completion
from .schema import encode


class MockError(WardError):
    pass


class Raw:
    """An answer given as JSON text as-is, e.g. to test invalid output."""

    def __init__(self, text: str) -> None:
        self.text = text


class Usage:
    """An answer with what it cost: `Usage("yes", tokens=500, cost=0.01)`."""

    def __init__(self, answer: Any, tokens: int | None = None, cost: float | None = 0.0) -> None:
        self.answer = answer
        self.tokens = tokens
        self.cost = cost


class Seq:
    """Answers given one per call, in order. Running out is an error."""

    def __init__(self, *answers: Any) -> None:
        self.answers = list(answers)


class MockModel:
    """Answers each `ai fn` from `answers`, keyed by function name. An answer is a
    value (encoded as JSON, so records and enums work), a `Raw` text, a `Seq` of
    answers, a `Usage` with what the answer cost, or a callable taking the `AiRequest`
    and returning one of those."""

    def __init__(self, answers: Mapping[str, Any] | None = None) -> None:
        self.answers = dict(answers or {})
        self.calls: list[AiRequest] = []
        self._next: dict[str, int] = {}

    @classmethod
    def from_json(cls, text: str) -> MockModel:
        """`{"fn_name": answer, ...}`, answers as JSON values."""
        answers = json.loads(text)
        if not isinstance(answers, dict):
            raise MockError("mock answers must be a JSON object keyed by function name")
        return cls(answers)

    def complete(self, request: AiRequest) -> str | Completion:
        self.calls.append(request)
        if request.function not in self.answers:
            raise MockError(f"the mock model has no answer for `{request.function}`")
        answer = self._text(request, self.answers[request.function])
        # Mock answers cost nothing (unless a `Usage` says otherwise), so `cost` budgets
        # can run against the mock.
        return Completion(answer, None, 0.0) if isinstance(answer, str) else answer

    def _text(self, request: AiRequest, answer: Any) -> str | Completion:
        if isinstance(answer, Seq):
            n = self._next.get(request.function, 0)
            if n >= len(answer.answers):
                raise MockError(
                    f"the mock model ran out of answers for `{request.function}` "
                    f"after {len(answer.answers)}"
                )
            self._next[request.function] = n + 1
            return self._text(request, answer.answers[n])
        if isinstance(answer, Usage):
            inner = self._text(request, answer.answer)
            text = inner.text if isinstance(inner, Completion) else inner
            return Completion(text, answer.tokens, answer.cost)
        if isinstance(answer, Raw):
            return answer.text
        if callable(answer) and not isinstance(answer, type):
            return self._text(request, answer(request))
        return json.dumps(encode(answer))


__all__ = ["MockError", "MockModel", "Raw", "Seq", "Usage"]
