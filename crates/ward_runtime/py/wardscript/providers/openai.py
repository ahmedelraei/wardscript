"""Models through the OpenAI Chat Completions API, with the return type's schema as the
response format; usage is reported to budgets."""

from __future__ import annotations

import json
from typing import Any

from ..model import AiRequest, Completion
from . import cost, object_schema, prompt_text


class OpenAI:
    def __init__(
        self,
        model: str,
        *,
        prices: tuple[float, float] | None = None,
        client: Any = None,
    ) -> None:
        """`client` defaults to `openai.OpenAI()`, which reads `OPENAI_API_KEY`."""
        if client is None:
            import openai

            client = openai.OpenAI()
        self.client = client
        self.model = model
        self.prices = prices

    def complete(self, request: AiRequest) -> Completion:
        response = self.client.chat.completions.create(
            model=self.model,
            messages=[{"role": "user", "content": prompt_text(request)}],
            response_format={
                "type": "json_schema",
                # Not strict: strict mode rejects parts of JSON Schema that answers use.
                "json_schema": {"name": "answer", "schema": object_schema(request.schema)},
            },
        )
        content = response.choices[0].message.content or ""
        try:
            text = json.dumps(json.loads(content)["value"], ensure_ascii=False)
        except (json.JSONDecodeError, KeyError, TypeError):
            text = content
        usage = response.usage
        return Completion(
            text,
            tokens=usage.prompt_tokens + usage.completion_tokens,
            cost=cost(self.prices, usage.prompt_tokens, usage.completion_tokens),
        )
