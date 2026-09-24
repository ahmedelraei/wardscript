"""The audit trace. A run starts when the host calls into Wardscript and ends when that
call returns; the runtime records its model and tool calls and every `validate`,
`approve` and `declassify` through the runtime core, which writes one JSON line per
event to `<trace_dir>/<run id>.jsonl`. `ward trace show <run id>` reads it back."""

from __future__ import annotations

import contextlib
import contextvars
import dataclasses
import json
import os
import time
from typing import Any, Iterator

from . import core
from .errors import Thrown
from .trust import Trusted


@dataclasses.dataclass
class Run:
    recorder: Any
    records: list[dict] = dataclasses.field(default_factory=list)
    tokens: float = 0.0
    calls: float = 0.0
    cost: float = 0.0

    @property
    def id(self) -> str:
        return self.recorder.run

    @property
    def path(self) -> str | None:
        return self.recorder.path


_current: contextvars.ContextVar[Run | None] = contextvars.ContextVar("wardscript_run", default=None)
_last: Run | None = None


def current() -> Run | None:
    return _current.get()


def last() -> Run | None:
    """The most recent run that finished."""
    return _last


def to_json(value: Any) -> Any:
    """A JSON value for anything, for the trace: Wardscript values encode exactly, and
    other host values as well as they can."""
    from .schema import encode

    if isinstance(value, Trusted):
        return to_json(value.value)
    try:
        return encode(value)
    except Exception:
        pass
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {f.name: to_json(getattr(value, f.name)) for f in dataclasses.fields(value)}
    if isinstance(value, (list, tuple)):
        return [to_json(x) for x in value]
    if isinstance(value, dict):
        return {str(k): to_json(v) for k, v in value.items()}
    return repr(value)


def _text(value: Any) -> str:
    return json.dumps(to_json(value), ensure_ascii=False)


def digest(value: Any) -> str:
    return core.digest(_text(value))


def leaves(value: Any) -> list[dict[str, str]]:
    return [{"path": p, "digest": d} for p, d in core.leaves(_text(value))]


def record(kind: str, **fields: Any) -> None:
    run = _current.get()
    if run is None:
        return
    line = run.recorder.record(json.dumps({"kind": kind, **fields}, ensure_ascii=False))
    run.records.append(json.loads(line))


def _trace_dir() -> str | None:
    from .runtime import config

    return config().trace_dir or os.environ.get("WARD_TRACE_DIR") or None


@contextlib.contextmanager
def call(function: str, args: list[tuple[str, Any]]) -> Iterator[None]:
    """Entered by every generated function; the outermost one is a run."""
    if _current.get() is not None:
        yield
        return
    global _last
    run = Run(core.Recorder(_trace_dir()))
    token = _current.set(run)
    arguments = [
        {
            "name": name,
            "value": to_json(value),
            "vouched": isinstance(value, Trusted),
            "leaves": leaves(value),
        }
        for name, value in args
    ]
    record("run_start", function=function, args=arguments)
    status, error = "ok", None
    try:
        yield
    except Thrown as e:
        status, error = "threw", _text(e.value)
        raise
    except BaseException as e:
        status, error = "error", f"{type(e).__name__}: {e}"
        raise
    finally:
        record("run_end", status=status, error=error, tokens=run.tokens, calls=run.calls, cost=run.cost)
        _current.reset(token)
        _last = run


def now() -> int:
    return time.time_ns()
