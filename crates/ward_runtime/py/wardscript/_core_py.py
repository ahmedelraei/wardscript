"""Pure-Python fallback for `wardscript._core`, the Rust runtime core. Same interface and
the same trace format; used when the package wasn't built with its native module, e.g. by
`ward run`, which embeds the package as source."""

from __future__ import annotations

import json
import os
import time
from typing import Any

IMPLEMENTATION = "python"
_MAX_LEAVES = 512


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def digest(value: str) -> str:
    return _digest(json.loads(value))


def _digest(value: Any) -> str:
    h = 0xCBF29CE484222325
    for b in _canonical(value).encode("utf-8"):
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


def leaves(value: str) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []

    def walk(v: Any, path: str) -> None:
        if len(out) >= _MAX_LEAVES:
            return
        out.append((path, _digest(v)))
        if isinstance(v, list):
            for i, x in enumerate(v):
                walk(x, f"{path}[{i}]")
        elif isinstance(v, dict):
            for k in sorted(v):
                walk(v[k], f"{path}.{k}")

    walk(json.loads(value), "$")
    return out


class Budget:
    def __init__(
        self,
        function: str,
        tokens: float | None = None,
        calls: float | None = None,
        cost: float | None = None,
        time: float | None = None,
    ) -> None:
        self.function = function
        self._limits = {"tokens": tokens, "calls": calls, "cost": cost, "time": time}
        self._used = {"tokens": 0.0, "calls": 0.0, "cost": 0.0}
        self._started = _monotonic()

    @property
    def used(self) -> tuple[float, float, float]:
        return (self._used["tokens"], self._used["calls"], self._used["cost"])

    def _over(self, resource: str, used: float) -> tuple[str, float, float] | None:
        limit = self._limits[resource]
        return (resource, float(limit), used) if limit is not None and used > limit else None

    def _check(self) -> tuple[str, float, float] | None:
        for r in ("tokens", "calls", "cost"):
            o = self._over(r, self._used[r])
            if o:
                return o
        return self.check_time()

    def charge_call(self) -> tuple[str, float, float] | None:
        self._used["calls"] += 1
        return self._check()

    def charge_usage(self, tokens: float, cost: float) -> tuple[str, float, float] | None:
        self._used["tokens"] += tokens
        self._used["cost"] += cost
        return self._check()

    def check_time(self) -> tuple[str, float, float] | None:
        return self._over("time", _monotonic() - self._started)


def _monotonic() -> float:
    return time.monotonic()


def _new_run_id() -> str:
    nanos = time.time_ns()
    mix = (nanos ^ (os.getpid() * 2654435761)) & 0xFFFF
    return f"{nanos // 1_000_000:011x}{mix:04x}"


# Field order of each event, as the Rust core serializes it. Floats stay floats so both
# cores write byte-identical lines.
_EVENTS = {
    "run_start": ("function", "args"),
    "run_end": ("status", "error", "tokens", "calls", "cost"),
    "ai_call": ("started", "function", "attempt", "prompt", "answer", "tokens", "cost", "error", "leaves"),
    "tool_call": ("started", "tool", "function", "site", "args", "digests", "error", "leaves"),
    "validate": ("rule", "site", "passed", "leaves"),
    "approve": ("site", "approved", "leaves"),
    "declassify": ("site", "reason", "leaves"),
    "budget_exceeded": ("function", "resource", "limit", "used"),
}
_FLOATS = {"tokens", "calls", "cost", "limit", "used"}


class Recorder:
    def __init__(self, dir: str | os.PathLike[str] | None = None) -> None:
        self.run = _new_run_id()
        self._seq = 0
        self.path: str | None = None
        self._file = None
        if dir is not None:
            os.makedirs(dir, exist_ok=True)
            self.path = os.path.join(os.fspath(dir), f"{self.run}.jsonl")
            self._file = open(self.path, "w", encoding="utf-8")  # noqa: SIM115

    def record(self, event: str) -> str:
        e = json.loads(event)
        kind = e.get("kind")
        if kind not in _EVENTS:
            raise ValueError(f"unknown event kind {kind!r}")
        record: dict[str, Any] = {"run": self.run, "seq": self._seq, "time": time.time_ns(), "kind": kind}
        for field in _EVENTS[kind]:
            if field not in e:
                raise ValueError(f"missing field `{field}`")
            v = e[field]
            record[field] = float(v) if field in _FLOATS and v is not None and kind != "tool_call" else v
        self._seq += 1
        line = json.dumps(record, separators=(",", ":"), ensure_ascii=False)
        if self._file is not None:
            self._file.write(line + "\n")
            self._file.flush()
        return line
