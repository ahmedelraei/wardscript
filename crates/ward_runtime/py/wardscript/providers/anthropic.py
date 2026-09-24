"""Claude, through the Anthropic API. The return type's schema is given as a tool the
model must call, so answers are structured; usage is reported to budgets."""

from __future__ import annotations

import json
from typing import Any

from ..model import AiRequest, Completion
from . import cost, object_schema, prompt_text

DEFAULT_MODEL = "claude-sonnet-5"


class Anthropic:
    def __init__(
        self,
        model: str | None = None,
        *,
        max_tokens: int = 4096,
        prices: tuple[float, float] | None = None,
        client: Any = None,
    ) -> None:
        """`prices` are dollars per million input and output tokens, for `cost` budgets;
        without them every call costs 0. `client` defaults to `anthropic.Anthropic()`,
        which reads `ANTHROPIC_API_KEY`."""
        if client is None:
            import anthropic

            client = anthropic.Anthropic()
        self.client = client
        self.model = model or DEFAULT_MODEL
        self.max_tokens = max_tokens
        self.prices = prices

    def complete(self, request: AiRequest) -> Completion:
        message = self.client.messages.create(
            model=self.model,
            max_tokens=self.max_tokens,
            messages=[{"role": "user", "content": prompt_text(request)}],
            tools=[
                {
                    "name": "answer",
                    "description": f"Give the answer of `{request.function}`.",
                    "input_schema": object_schema(request.schema),
                }
            ],
            tool_choice={"type": "tool", "name": "answer"},
        )
        answer = next((b for b in message.content if getattr(b, "type", None) == "tool_use"), None)
        if answer is not None and isinstance(answer.input, dict) and "value" in answer.input:
            text = json.dumps(answer.input["value"], ensure_ascii=False)
        else:
            # No tool call: whatever text there is gets decoded, and rejected if it's wrong.
            text = "".join(getattr(b, "text", "") for b in message.content)
        usage = message.usage
        return Completion(
            text,
            tokens=usage.input_tokens + usage.output_tokens,
            cost=cost(self.prices, usage.input_tokens, usage.output_tokens),
        )
