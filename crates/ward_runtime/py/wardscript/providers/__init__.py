"""Model providers: `Model`s backed by real LLM APIs. Each needs its SDK installed
(`pip install wardscript[anthropic]`) and reads its API key from the environment.

    from wardscript import runtime
    from wardscript.providers.anthropic import Anthropic

    runtime.configure(model=Anthropic("claude-sonnet-5", prices=(3.0, 15.0)))
"""

from __future__ import annotations

import copy
from typing import Any

from ..model import Model

PROVIDERS = ("anthropic", "openai")


def load(spec: str) -> Model:
    """A provider from `name` or `name:model`, e.g. `anthropic:claude-sonnet-5`."""
    name, _, model = spec.partition(":")
    if name == "anthropic":
        from .anthropic import Anthropic

        return Anthropic(model or None)
    if name == "openai":
        from .openai import OpenAI

        if not model:
            raise ValueError("give the OpenAI model to use, like `openai:<model>`")
        return OpenAI(model)
    raise ValueError(f"unknown model provider `{name}`; known: {', '.join(PROVIDERS)}")


def object_schema(schema: dict) -> dict:
    """APIs want an object at the top of a structured-output schema, so the answer is
    wrapped as `{"value": ...}`; `$defs` stay at the top so `$ref`s still resolve."""
    inner = copy.deepcopy(schema)
    defs = inner.pop("$defs", None)
    wrapped: dict[str, Any] = {
        "type": "object",
        "properties": {"value": inner},
        "required": ["value"],
        "additionalProperties": False,
    }
    if defs:
        wrapped["$defs"] = defs
    return wrapped


def cost(prices: tuple[float, float] | None, input_tokens: int, output_tokens: int) -> float | None:
    """Dollars, from prices per million input and output tokens; unknown without prices."""
    if prices is None:
        return None
    return (input_tokens * prices[0] + output_tokens * prices[1]) / 1_000_000


def prompt_text(request: Any) -> str:
    """The prompt, plus why the last answer was rejected when retrying."""
    text = request.prompt
    if request.errors:
        text += "\n\nYour previous answer was rejected: " + request.errors[-1]
    return text


__all__ = ["PROVIDERS", "load"]
