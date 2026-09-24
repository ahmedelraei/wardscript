"""A deterministic model for tests: scripted answers per `ai fn`."""

from __future__ import annotations

import json
from typing import Any, Mapping

from .errors import WardError
from .model import AiRequest
from .schema import encode


class MockError(WardError):
    pass


class Raw:
    """An answer given as JSON text as-is, e.g. to test invalid output."""

    def __init__(self, text: str) -> None:
        self.text = text


class Seq:
    """Answers given one per call, in order. Running out is an error."""

    def __init__(self, *answers: Any) -> None:
        self.answers = list(answers)


class MockModel:
    """Answers each `ai fn` from `answers`, keyed by function name. An answer is a
    value (encoded as JSON, so records and enums work), a `Raw` text, a `Seq` of
    answers, or a callable taking the `AiRequest` and returning one of those."""

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

    def complete(self, request: AiRequest) -> str:
        self.calls.append(request)
        if request.function not in self.answers:
            raise MockError(f"the mock model has no answer for `{request.function}`")
        return self._text(request, self.answers[request.function])

    def _text(self, request: AiRequest, answer: Any) -> str:
        if isinstance(answer, Seq):
            n = self._next.get(request.function, 0)
            if n >= len(answer.answers):
                raise MockError(
                    f"the mock model ran out of answers for `{request.function}` "
                    f"after {len(answer.answers)}"
                )
            self._next[request.function] = n + 1
            return self._text(request, answer.answers[n])
        if isinstance(answer, Raw):
            return answer.text
        if callable(answer) and not isinstance(answer, type):
            return self._text(request, answer(request))
        return json.dumps(encode(answer))


__all__ = ["MockError", "MockModel", "Raw", "Seq"]
