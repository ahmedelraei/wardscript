"""Running `test` blocks. `ward test` replays the model answers and tool results each
test got when it was recorded, so tests run offline and give the same result every
time; `ward test --record` runs them against a model (and the tools in `mcp.json`)
and writes what they got to the recordings file.

A recording is strict: a test that asks a different function, a different prompt,
or calls a tool with different arguments than when it was recorded fails, and so
does one that makes fewer or more calls. Record it again after changing it.
"""

from __future__ import annotations

import asyncio
import contextvars
import inspect
import json
import os
import sys
from typing import Any, Callable, Mapping, Sequence

from . import audit, runtime
from .errors import (
    ModelError,
    ModelUnavailable,
    RateLimited,
    TestFailure,
    Thrown,
    ToolError,
    WardError,
)
from .model import Completion

VERSION = 1


class RecordingMismatch(WardError):
    """What a test did doesn't match its recording."""


class _Tape:
    def __init__(self, test: str, events: list[dict], replay: bool) -> None:
        self.test = test
        self.events = events
        self.replay = replay
        self.pos = 0

    def next(self, what: str) -> dict:
        if self.pos >= len(self.events):
            raise RecordingMismatch(
                f"test `{self.test}` {what}, but its recording ends here; run `ward test --record`"
            )
        event = self.events[self.pos]
        self.pos += 1
        return event

    def mismatch(self, what: str) -> RecordingMismatch:
        return RecordingMismatch(
            f"test `{self.test}` doesn't match its recording: {what}; run `ward test --record`"
        )

    def add(self, event: dict) -> None:
        self.events.append(event)


_tape: contextvars.ContextVar[_Tape | None] = contextvars.ContextVar("wardscript_tape", default=None)


def _current() -> _Tape:
    tape = _tape.get()
    if tape is None:
        raise WardError("recorded models and tools only work inside `ward test`")
    return tape


def _then(value: Any, f: Callable[[Any], Any]) -> Any:
    """`f(value)`, after awaiting `value` if it's awaitable."""
    if inspect.isawaitable(value):

        async def wait() -> Any:
            return f(await value)

        return wait()
    return f(value)


# -- models --

_ERRORS: dict[str, type[ModelError]] = {
    "ModelError": ModelError,
    "RateLimited": RateLimited,
    "ModelUnavailable": ModelUnavailable,
}


class ReplayModel:
    """Answers from the recording. One per model alias; `None` is the default model."""

    def __init__(self, alias: str | None = None) -> None:
        self.alias = alias

    def complete(self, request: Any) -> Completion:
        tape = _current()
        e = tape.next(f"asked the model for `{request.function}`")
        if e.get("kind") != "model" or e.get("function") != request.function:
            raise tape.mismatch(f"`{request.function}` asked the model where the recording has {_describe(e)}")
        if e.get("model") != self.alias:
            raise tape.mismatch(f"`{request.function}` asked the model `{self.alias}`, not `{e.get('model')}`")
        if e.get("prompt") != request.prompt:
            raise tape.mismatch(f"`{request.function}` was asked a different prompt")
        if "error" in e:
            err = e["error"]
            raise _ERRORS.get(err.get("type"), ModelError)(err.get("message", ""), status=err.get("status"))
        return Completion(e["text"], e.get("tokens"), e.get("cost"))


class RecordingModel:
    """Asks `inner` and records what it answered."""

    def __init__(self, inner: Any, alias: str | None = None) -> None:
        self.inner = inner
        self.alias = alias
        if hasattr(inner, "prices"):
            self.prices = inner.prices

    def complete(self, request: Any) -> Any:
        event = {"kind": "model", "function": request.function, "model": self.alias, "prompt": request.prompt}
        try:
            answer = self.inner.complete(request)
        except ModelError as e:
            self._error(event, e)
            raise
        if inspect.isawaitable(answer):
            return self._wait(event, answer)
        return self._add(event, answer)

    async def _wait(self, event: dict, answer: Any) -> Any:
        try:
            value = await answer
        except ModelError as e:
            self._error(event, e)
            raise
        return self._add(event, value)

    def _error(self, event: dict, e: ModelError) -> None:
        event["error"] = {"type": type(e).__name__, "message": str(e), "status": e.status}
        _current().add(event)

    def _add(self, event: dict, answer: Any) -> Any:
        if isinstance(answer, Completion):
            event.update(text=answer.text, tokens=answer.tokens, cost=answer.cost)
        else:
            event.update(text=answer, tokens=None, cost=None)
        _current().add(event)
        return answer


def _describe(e: dict) -> str:
    if e.get("kind") == "model":
        return f"a model call of `{e.get('function')}`"
    if e.get("kind") == "tool":
        return f"a call of `{e.get('tool')}.{e.get('function')}`"
    return "something else"


# -- tools --


def _tool_result(e: dict) -> Any:
    if "thrown" in e:
        raise Thrown(e["thrown"])
    if "error" in e:
        raise ToolError(e["error"])
    return e.get("result")


class ReplayTools:
    """The results `source`'s tools gave when the test was recorded."""

    def __init__(self, source: str) -> None:
        self.source = source

    def _next(self, name: str, key: str, value: Any) -> Any:
        tape = _current()
        e = tape.next(f"called `{self.source}.{name}`")
        if e.get("kind") != "tool" or (e.get("tool"), e.get("function")) != (self.source, name):
            raise tape.mismatch(f"`{self.source}.{name}` was called where the recording has {_describe(e)}")
        if e.get(key) != value:
            raise tape.mismatch(f"`{self.source}.{name}` got different arguments")
        return _tool_result(e)

    def call_tool(self, name: str, arguments: Mapping[str, Any]) -> Any:
        return self._next(name, "arguments", audit.to_json(dict(arguments)))

    def __getattr__(self, name: str) -> Callable[..., Any]:
        if name.startswith("__"):
            raise AttributeError(name)
        return lambda *args: self._next(name, "args", [audit.to_json(a) for a in args])


class RecordingTools:
    """Calls `inner` (a tool implementation, as `configure(tools=...)` takes) and
    records the results."""

    def __init__(self, source: str, inner: Any) -> None:
        self.source = source
        self.inner = inner

    def _function(self, name: str) -> Callable[..., Any]:
        inner = self.inner
        fn = inner.get(name) if isinstance(inner, Mapping) else getattr(inner, name, None)
        if fn is None:
            raise ToolError(f"tool `{self.source}` has no function `{name}` to record")
        return fn

    def _record(self, event: dict, call: Callable[[], Any]) -> Any:
        tape = _current()
        try:
            result = call()
        except Thrown as e:
            event["thrown"] = audit.to_json(e.value)
            tape.add(event)
            raise
        except ToolError as e:
            event["error"] = str(e)
            tape.add(event)
            raise

        def done(value: Any) -> Any:
            event["result"] = audit.to_json(value)
            tape.add(event)
            return value

        return _then(result, done)

    def call_tool(self, name: str, arguments: Mapping[str, Any]) -> Any:
        event = {"kind": "tool", "tool": self.source, "function": name, "arguments": audit.to_json(dict(arguments))}
        named = getattr(self.inner, "call_tool", None)
        if callable(named):
            return self._record(event, lambda: named(name, arguments))
        return self._record(event, lambda: self._function(name)(*arguments.values()))

    def __getattr__(self, name: str) -> Callable[..., Any]:
        if name.startswith("__"):
            raise AttributeError(name)

        def call(*args: Any) -> Any:
            event = {"kind": "tool", "tool": self.source, "function": name, "args": [audit.to_json(a) for a in args]}
            return self._record(event, lambda: self._function(name)(*args))

        return call


# -- recordings --


def load(path: str) -> dict[str, list[dict]]:
    if not os.path.exists(path):
        return {}
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if data.get("version") != VERSION:
        raise WardError(f"`{path}` has recordings version {data.get('version')}; this runtime reads {VERSION}")
    return dict(data.get("tests", {}))


def save(path: str, tests: Mapping[str, list[dict]]) -> None:
    data = {"version": VERSION, "tests": {name: tests[name] for name in sorted(tests)}}
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")


# -- running --

Test = tuple[str, str, Callable[[], Any]]


def run(
    tests: Sequence[Test],
    *,
    recordings: str,
    record: bool = False,
    filters: Sequence[str] = (),
    aliases: Sequence[str] = (),
    sources: Sequence[str] = (),
    model: Any = None,
    models: Mapping[str, Any] | None = None,
    tools: Mapping[str, Any] | None = None,
    out: Any = None,
) -> bool:
    """Runs the tests whose names contain one of `filters` (all without filters),
    printing a line each, and returns whether all passed. `model`, `models` (by
    alias; `model` for the rest) and `tools` are what a recording run asks; a replay
    uses the recordings only."""
    out = out or sys.stdout
    selected = [t for t in tests if not filters or any(f in t[0] for f in filters)]
    tapes = load(recordings)
    if record:
        inner = {a: (models or {}).get(a, model) for a in aliases}
        runtime.configure(
            model=RecordingModel(model) if model is not None else None,
            models={a: RecordingModel(m, a) for a, m in inner.items() if m is not None},
            tools={s: RecordingTools(s, (tools or {}).get(s)) for s in sources},
        )
    else:
        runtime.configure(
            model=ReplayModel(),
            models={a: ReplayModel(a) for a in aliases},
            tools={s: ReplayTools(s) for s in sources},
        )
    # Tests approve whatever asks; a recording holds no human in the loop.
    runtime.configure(approver=lambda request: True)
    failed = 0
    for name, site, fn in selected:
        if not record and name not in tapes:
            failed += 1
            print(f"test {name} ... FAILED", file=out)
            print(f"    no recording for this test ({site}); run `ward test --record`", file=out)
            continue
        tape = _Tape(name, [] if record else list(tapes[name]), replay=not record)
        token = _tape.set(tape)
        problem = None
        try:
            result = fn()
            if inspect.isawaitable(result):
                asyncio.run(result)
            if not record and tape.pos < len(tape.events):
                problem = tape.mismatch(f"it made {tape.pos} calls, and {len(tape.events)} were recorded")
        except TestFailure as e:
            problem = e
        except Thrown as e:
            problem = WardError(f"threw {json.dumps(audit.to_json(e.value), ensure_ascii=False)}")
        except WardError as e:
            problem = e
        finally:
            _tape.reset(token)
        if record:
            tapes[name] = tape.events
        if problem is None:
            print(f"test {name} ... ok", file=out)
        else:
            failed += 1
            print(f"test {name} ... FAILED", file=out)
            print(f"    {problem}", file=out)
    if record:
        save(recordings, tapes)
    skipped = len(tests) - len(selected)
    summary = f"{len(selected) - failed} passed, {failed} failed"
    if skipped:
        summary += f", {skipped} filtered out"
    print(summary, file=out)
    return failed == 0


__all__ = ["RecordingMismatch", "ReplayModel", "RecordingModel", "ReplayTools", "RecordingTools", "load", "run", "save"]
