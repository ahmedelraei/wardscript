"""The interface between `ai fn`s and language models."""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True)
class AiRequest:
    #: The `ai fn` being called.
    function: str
    #: Its prompt, with the arguments filled in.
    prompt: str
    #: JSON Schema the answer must match.
    schema: dict
    #: 0 for the first try; retries count up.
    attempt: int = 0
    #: Why each earlier answer was rejected.
    errors: tuple[str, ...] = ()

    def instructions(self) -> str:
        """The prompt plus the output contract, for providers without native JSON-schema
        support."""
        text = (
            f"{self.prompt}\n\nAnswer with only a JSON value matching this JSON Schema:\n"
            f"{json.dumps(self.schema)}"
        )
        if self.errors:
            text += "\n\nYour previous answer was rejected: " + self.errors[-1]
        return text


class Model(Protocol):
    def complete(self, request: AiRequest) -> str:
        """Returns the model's answer: JSON text."""
        ...
