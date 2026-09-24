"""Claude, through the Anthropic API. The return type's schema is given as a tool the
model must call, so answers are structured; usage is reported to budgets."""

from __future__ import annotations

import json
from typing import Any, Iterator

from ..model import AiRequest, Completion
from . import cost, object_schema, prompt_text, provider_errors

DEFAULT_MODEL = "claude-sonnet-5"


class Anthropic:
    #: `stream` yields the tool input, `{"value": ...}`.
    stream_wraps_value = True

    def __init__(
        self,
        model: str | None = None,
        *,
        max_tokens: int = 4096,
        prices: tuple[float, float] | None = None,
        client: Any = None,
    ) -> None:
        """`prices` are dollars per million input and output tokens, for `cost` budgets;
        without them the cost is unknown, and a `cost` budget refuses the call. `client` defaults to `anthropic.Anthropic()`,
        which reads `ANTHROPIC_API_KEY`, without the SDK's own retries: the runtime
        retries by the `ai fn`'s model policy."""
        if client is None:
            import anthropic

            client = anthropic.Anthropic(max_retries=0)
        self.client = client
        self.model = model or DEFAULT_MODEL
        self.max_tokens = max_tokens
        self.prices = prices

    def _request(self, request: AiRequest) -> dict[str, Any]:
        return {
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": prompt_text(request)}],
            "tools": [
                {
                    "name": "answer",
                    "description": f"Give the answer of `{request.function}`.",
                    "input_schema": object_schema(request.schema),
                }
            ],
            "tool_choice": {"type": "tool", "name": "answer"},
        }

    def _completion(self, tool_input: Any, text: str, input_tokens: int, output_tokens: int) -> Completion:
        if isinstance(tool_input, dict) and "value" in tool_input:
            text = json.dumps(tool_input["value"], ensure_ascii=False)
        # Else no tool call: whatever text there is gets decoded, and rejected if wrong.
        return Completion(
            text,
            tokens=input_tokens + output_tokens,
            cost=cost(self.prices, input_tokens, output_tokens),
        )

    def complete(self, request: AiRequest) -> Completion:
        with provider_errors():
            message = self.client.messages.create(**self._request(request))
        answer = next((b for b in message.content if getattr(b, "type", None) == "tool_use"), None)
        text = "".join(getattr(b, "text", "") for b in message.content)
        usage = message.usage
        return self._completion(
            answer.input if answer is not None else None, text, usage.input_tokens, usage.output_tokens
        )

    def stream(self, request: AiRequest) -> Iterator[str | Completion]:
        """Yields the tool call's JSON (`{"value": ...}`) as it arrives, then the answer."""
        with provider_errors():
            yield from self._stream(request)

    def _stream(self, request: AiRequest) -> Iterator[str | Completion]:
        events = self.client.messages.create(**self._request(request), stream=True)
        tool_json, text, input_tokens, output_tokens = "", "", 0, 0
        for event in events:
            kind = getattr(event, "type", None)
            if kind == "message_start":
                input_tokens = event.message.usage.input_tokens
                output_tokens = getattr(event.message.usage, "output_tokens", 0) or 0
            elif kind == "message_delta":
                output_tokens = event.usage.output_tokens
            elif kind == "content_block_delta":
                delta = event.delta
                if delta.type == "input_json_delta":
                    tool_json += delta.partial_json
                    yield delta.partial_json
                elif delta.type == "text_delta":
                    text += delta.text
                    yield delta.text
        try:
            tool_input = json.loads(tool_json) if tool_json else None
        except json.JSONDecodeError:
            tool_input = None
        yield self._completion(tool_input, text or tool_json, input_tokens, output_tokens)
