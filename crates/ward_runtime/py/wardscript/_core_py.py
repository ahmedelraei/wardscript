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

    def charge_usage(self, tokens: float, cost: float | None = None) -> tuple[str, float, float] | None:
        self._used["tokens"] += tokens
        self._used["cost"] += 0.0 if cost is None else cost
        return self._check()

    @property
    def limits_cost(self) -> bool:
        return self._limits["cost"] is not None

    def unenforceable(self, cost: float | None = None) -> bool:
        return cost is None and self.limits_cost

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
    "ai_call": ("started", "function", "attempt", "model", "prompt", "answer", "tokens", "cost", "error", "leaves"),
    "tool_call": ("started", "tool", "function", "site", "args", "digests", "error", "leaves"),
    "validate": ("rule", "site", "passed", "leaves"),
    "approve": ("site", "approved", "leaves"),
    "declassify": ("site", "reason", "leaves"),
    "budget_exceeded": ("function", "resource", "limit", "used"),
    "budget_unenforceable": ("function", "when"),
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

    def close(self) -> None:
        if self._file is not None:
            self._file.close()
            self._file = None

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


def _attr(key: str, value: Any) -> dict[str, Any]:
    if isinstance(value, str):
        v: dict[str, Any] = {"stringValue": value}
    elif isinstance(value, bool):
        v = {"boolValue": value}
    elif isinstance(value, int):
        v = {"intValue": str(value)}
    elif isinstance(value, float):
        v = {"doubleValue": value}
    else:
        v = {"stringValue": json.dumps(value, separators=(",", ":"), ensure_ascii=False)}
    return {"key": key, "value": v}


def _status(error: str | None) -> dict[str, Any]:
    return {"code": 1} if error is None else {"code": 2, "message": error}


def otlp(records: str) -> str:
    """A run's records (a JSON list) as OTLP/JSON spans; see `ward_runtime::otlp`."""
    recs = json.loads(records)
    run = recs[0]["run"] if recs else ""
    trace_id = _digest(run) + _digest([run])
    root = _digest([run, 0])
    spans: list[dict[str, Any]] = []
    events: list[dict[str, Any]] = []
    name, start, end, status = "", 0, 0, {}
    for r in recs:
        kind, t = r["kind"], str(r["time"])
        if kind == "run_start":
            name, start = f"run {r['function']}", r["time"]
        elif kind == "run_end":
            end = r["time"]
            status = {"code": 1} if r["status"] == "ok" else {"code": 2, "message": r["error"] or ""}
            usage = [_attr("ward.tokens", r["tokens"]), _attr("ward.calls", r["calls"]), _attr("ward.cost", r["cost"])]
            events.append({"timeUnixNano": t, "name": "usage", "attributes": usage})
        elif kind in ("ai_call", "tool_call"):
            if kind == "ai_call":
                span_name = f"ai fn {r['function']}"
                attrs = [
                    _attr("ward.attempt", r["attempt"]),
                    _attr("gen_ai.usage.total_tokens", r["tokens"]),
                ]
                if r["model"] is not None:
                    attrs.append(_attr("gen_ai.request.model", r["model"]))
                if r["cost"] is not None:
                    attrs.append(_attr("ward.cost", r["cost"]))
            else:
                span_name = f"{r['tool']}.{r['function']}"
                attrs = [_attr("ward.site", r["site"])]
            spans.append(
                {
                    "traceId": trace_id,
                    "spanId": _digest([run, r["seq"]]),
                    "parentSpanId": root,
                    "name": span_name,
                    "kind": 3,
                    "startTimeUnixNano": str(r["started"]),
                    "endTimeUnixNano": t,
                    "attributes": attrs,
                    "status": _status(r["error"]),
                }
            )
        elif kind == "validate":
            attrs = [_attr("ward.rule", r["rule"]), _attr("ward.site", r["site"]), _attr("ward.passed", r["passed"])]
            events.append({"timeUnixNano": t, "name": "validate", "attributes": attrs})
        elif kind == "approve":
            attrs = [_attr("ward.site", r["site"]), _attr("ward.approved", r["approved"])]
            events.append({"timeUnixNano": t, "name": "approve", "attributes": attrs})
        elif kind == "declassify":
            attrs = [_attr("ward.site", r["site"]), _attr("ward.reason", r["reason"])]
            events.append({"timeUnixNano": t, "name": "declassify", "attributes": attrs})
        elif kind == "budget_exceeded":
            attrs = [
                _attr("ward.function", r["function"]),
                _attr("ward.resource", r["resource"]),
                _attr("ward.limit", r["limit"]),
                _attr("ward.used", r["used"]),
            ]
            events.append({"timeUnixNano": t, "name": "budget_exceeded", "attributes": attrs})
        elif kind == "budget_unenforceable":
            attrs = [_attr("ward.function", r["function"]), _attr("ward.when", r["when"])]
            events.append({"timeUnixNano": t, "name": "budget_unenforceable", "attributes": attrs})
    spans.insert(
        0,
        {
            "traceId": trace_id,
            "spanId": root,
            "name": name,
            "kind": 1,
            "startTimeUnixNano": str(start),
            "endTimeUnixNano": str(max(end, start)),
            "attributes": [_attr("ward.run", run)],
            "events": events,
            "status": status,
        },
    )
    out = {
        "resourceSpans": [
            {
                "resource": {"attributes": [_attr("service.name", "wardscript")]},
                "scopeSpans": [{"scope": {"name": "wardscript"}, "spans": spans}],
            }
        ]
    }
    return json.dumps(out, separators=(",", ":"), ensure_ascii=False)
