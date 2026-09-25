"""Models through the OpenAI Chat Completions API, with the return type's schema as the
response format; usage is reported to budgets."""

from __future__ import annotations

import json
from typing import Any, Iterator

from ..model import AiRequest, Completion
from . import cost, object_schema, prompt_text, provider_errors


class OpenAI:
    #: `stream` yields the response object, `{"value": ...}`.
    stream_wraps_value = True

    def __init__(
        self,
        model: str,
        *,
        prices: tuple[float, float] | None = None,
        client: Any = None,
    ) -> None:
        """`client` defaults to `openai.OpenAI()`, which reads `OPENAI_API_KEY`, without
        the SDK's own retries: the runtime retries by the `ai fn`'s model policy."""
        if client is None:
            import openai

            client = openai.OpenAI(max_retries=0)
        self.client = client
        self.model = model
        self.prices = prices

    def _request(self, request: AiRequest) -> dict[str, Any]:
        return {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt_text(request)}],
            "response_format": {
                "type": "json_schema",
                # Not strict: strict mode rejects parts of JSON Schema that answers use.
                "json_schema": {"name": "answer", "schema": object_schema(request.schema)},
            },
        }

    def _completion(self, content: str, usage: Any) -> Completion:
        try:
            text = json.dumps(json.loads(content)["value"], ensure_ascii=False)
        except (json.JSONDecodeError, KeyError, TypeError):
            text = content
        prompt_tokens = usage.prompt_tokens if usage else 0
        completion_tokens = usage.completion_tokens if usage else 0
        return Completion(
            text,
            tokens=prompt_tokens + completion_tokens if usage else None,
            cost=cost(self.prices, prompt_tokens, completion_tokens),
        )

    def complete(self, request: AiRequest) -> Completion:
        with provider_errors():
            response = self.client.chat.completions.create(**self._request(request))
        return self._completion(response.choices[0].message.content or "", response.usage)

    def stream(self, request: AiRequest) -> Iterator[str | Completion]:
        """Yields the answer's JSON (`{"value": ...}`) as it arrives, then the answer."""
        with provider_errors():
            yield from self._stream(request)

    def _stream(self, request: AiRequest) -> Iterator[str | Completion]:
        chunks = self.client.chat.completions.create(
            **self._request(request), stream=True, stream_options={"include_usage": True}
        )
        content, usage = "", None
        for chunk in chunks:
            if chunk.choices:
                delta = chunk.choices[0].delta.content or ""
                if delta:
                    content += delta
                    yield delta
            if getattr(chunk, "usage", None) is not None:
                usage = chunk.usage
        yield self._completion(content, usage)
