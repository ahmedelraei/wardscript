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
import urllib.request
import warnings
from typing import Any, Iterator

from . import core
from .errors import Thrown, TrustError
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


def _otlp_url() -> str | None:
    from .runtime import config

    base = config().otlp_endpoint or os.environ.get("OTEL_EXPORTER_OTLP_ENDPOINT")
    if not base:
        return None
    base = base.rstrip("/")
    return base if base.endswith("/v1/traces") else base + "/v1/traces"


def send_otlp(run: Run, url: str) -> None:
    """POSTs the run's spans to an OTLP/HTTP collector. A failure only warns: losing
    telemetry mustn't fail the run (the trace file still has it)."""
    body = core.otlp(json.dumps(run.records)).encode("utf-8")
    request = urllib.request.Request(
        url, data=body, headers={"Content-Type": "application/json"}, method="POST"
    )
    try:
        with urllib.request.urlopen(request, timeout=5) as response:
            response.read()
    except Exception as e:
        warnings.warn(f"wardscript: couldn't send run {run.id} to {url}: {e}", stacklevel=2)


# Leaves this short are too likely to match by chance, e.g. a literal "yes" in the
# program and a model answering "yes".
_MIN_TAINT_LEN = 8


def _untrusted_origin(run: Run, value: Any) -> str | None:
    """Where `value` came from, when it or one of its parts is exactly a value from an
    untrusted source in this run that no check passed and the host didn't vouch for."""
    sources: dict[str, str] = {}
    cleared: set[str] = set()
    for r in run.records:
        kind = r["kind"]
        if kind == "run_start":
            for a in r["args"]:
                if a["vouched"]:
                    cleared.update(leaf["digest"] for leaf in a["leaves"])
                else:
                    for leaf in a["leaves"]:
                        sources.setdefault(leaf["digest"], f"argument `{a['name']}` from the host")
        elif kind == "ai_call":
            for leaf in r["leaves"]:
                sources.setdefault(leaf["digest"], f"the output of `ai fn {r['function']}`")
        elif kind == "tool_call":
            for leaf in r["leaves"]:
                sources.setdefault(leaf["digest"], f"the result of `{r['tool']}.{r['function']}`")
        elif kind == "validate" and r["passed"]:
            cleared.update(leaf["digest"] for leaf in r["leaves"])
        elif kind == "approve" and r["approved"]:
            cleared.update(leaf["digest"] for leaf in r["leaves"])
        elif kind == "declassify":
            cleared.update(leaf["digest"] for leaf in r["leaves"])

    def walk(v: Any) -> str | None:
        if isinstance(v, str) and len(v) < _MIN_TAINT_LEN:
            return None
        if isinstance(v, (bool, int, float)) or v is None or v in ([], {}):
            return None
        d = core.digest(json.dumps(v, ensure_ascii=False))
        if d in cleared:
            return None
        if d in sources:
            return sources[d]
        parts = v.values() if isinstance(v, dict) else v if isinstance(v, list) else ()
        for x in parts:
            found = walk(x)
            if found:
                return found
        return None

    return walk(to_json(value))


def check_sink(tool: str, index: int, value: Any) -> None:
    """Defense in depth behind the compiler: refuses a tool argument that is, exactly, an
    unchecked untrusted value. Values combined from several sources aren't caught."""
    from .runtime import config

    run = _current.get()
    if run is None or not config().check_sinks:
        return
    origin = _untrusted_origin(run, value)
    if origin is not None:
        raise TrustError(
            f"argument {index + 1} of `{tool}` is {origin}, which no `validate`, `approve` "
            "or `declassify` checked; the tool was not called"
        )


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
        close = getattr(run.recorder, "close", None)
        if callable(close):
            close()
        url = _otlp_url()
        if url is not None:
            send_otlp(run, url)


def now() -> int:
    return time.time_ns()
