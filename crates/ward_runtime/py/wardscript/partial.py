"""Partial values of a streamed answer: the part of the value the model has written
so far, decoded as far as its type allows. `configure(on_partial=...)` receives them.

    runtime.configure(on_partial=lambda p: print(p.value))
    # Partial(Ticket, customer='Ada', summary='Checkout cr')
    # Partial(Ticket, customer='Ada', summary='Checkout crashes', priority=Priority.Urgent)

A record is a `Partial` with the fields that have started; a string may be cut off;
numbers, booleans and enum variants appear only once they're complete; a list holds
its elements so far.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any

from .schema import Adt, List, Map, Option, Prim, Refined, RecordInfo, Type, info, subst
from .values import some

#: Nothing usable yet.
MISSING: Any = type("Missing", (), {"__repr__": lambda self: "MISSING", "__bool__": lambda self: False})()


class _Cut(Exception):
    """The text ends inside a value."""


class _Prefix:
    """A lenient JSON parser for a prefix of a JSON text: objects, arrays and strings
    that haven't closed are returned as far as they go; a number, `true`, `false` or
    `null` that may still grow is left out."""

    def __init__(self, text: str) -> None:
        self.s = text
        self.i = 0

    def ws(self) -> None:
        while self.i < len(self.s) and self.s[self.i] in " \t\r\n":
            self.i += 1

    def value(self) -> Any:
        self.ws()
        if self.i >= len(self.s):
            raise _Cut
        c = self.s[self.i]
        if c == "{":
            return self.obj()
        if c == "[":
            return self.arr()
        if c == '"':
            return self.string()
        return self.scalar()

    def obj(self) -> dict:
        self.i += 1
        out: dict[str, Any] = {}
        while True:
            self.ws()
            if self.i >= len(self.s):
                return _Open(out)
            if self.s[self.i] == "}":
                self.i += 1
                return out
            if self.s[self.i] == ",":
                self.i += 1
                continue
            try:
                key = self.string()
            except _Cut:
                return _Open(out)
            if isinstance(key, _OpenStr):
                return _Open(out)
            self.ws()
            if self.i >= len(self.s) or self.s[self.i] != ":":
                return _Open(out)
            self.i += 1
            try:
                out[key] = self.value()
            except _Cut:
                return _Open(out)
            if isinstance(out[key], (_Open, _OpenStr)):
                return _Open(out)

    def arr(self) -> list:
        self.i += 1
        out: list[Any] = []
        while True:
            self.ws()
            if self.i >= len(self.s):
                return _OpenList(out)
            if self.s[self.i] == "]":
                self.i += 1
                return out
            if self.s[self.i] == ",":
                self.i += 1
                continue
            try:
                out.append(self.value())
            except _Cut:
                return _OpenList(out)
            if isinstance(out[-1], (_Open, _OpenStr, _OpenList)):
                return _OpenList(out)

    def string(self) -> Any:
        start = self.i
        self.i += 1
        chars = []
        while self.i < len(self.s):
            c = self.s[self.i]
            if c == '"':
                self.i += 1
                return json.loads(self.s[start : self.i])
            if c == "\\":
                if self.i + 1 >= len(self.s):
                    break
                if self.s[self.i + 1] == "u" and self.i + 6 > len(self.s):
                    break
                end = self.i + (6 if self.s[self.i + 1] == "u" else 2)
                chars.append(json.loads('"' + self.s[self.i : end] + '"'))
                self.i = end
                continue
            chars.append(c)
            self.i += 1
        self.i = len(self.s)
        return _OpenStr("".join(chars))

    def scalar(self) -> Any:
        start = self.i
        while self.i < len(self.s) and self.s[self.i] not in ",]} \t\r\n":
            self.i += 1
        if self.i >= len(self.s):
            raise _Cut  # It may continue: `12` could become `123`, `tr` `true`.
        return json.loads(self.s[start : self.i])


class _Open(dict):
    """An object that hasn't closed yet."""


class _OpenList(list):
    """An array that hasn't closed yet."""


class _OpenStr(str):
    """A string that hasn't closed yet."""


def parse_prefix(text: str) -> Any:
    """The JSON value `text` starts, as far as it goes, or `MISSING`."""
    try:
        return _Prefix(text).value()
    except (_Cut, json.JSONDecodeError, ValueError):
        return MISSING


@dataclass(frozen=True)
class Partial:
    """A record the model is still writing. Fields that haven't started are `None`."""

    cls: type
    fields: dict

    def __getattr__(self, name: str) -> Any:
        fields = self.__dict__.get("fields", {})
        if name in fields:
            return fields[name]
        raise AttributeError(name)

    def __repr__(self) -> str:
        shown = ", ".join(f"{k}={v!r}" for k, v in self.fields.items())
        return f"Partial({info(self.cls).name}{', ' if shown else ''}{shown})"


@dataclass(frozen=True)
class PartialValue:
    """What `configure(on_partial=...)` receives: `value` is `ai fn function`'s answer
    so far, in request `attempt` (retries restart from nothing)."""

    function: str
    attempt: int
    value: Any
    #: Whether the value is complete (the last one of each attempt).
    done: bool = False


def decode_partial(t: Type, v: Any) -> Any:
    """`v` (from `parse_prefix`) decoded as far as `t` allows; `MISSING` if nothing is."""
    if v is MISSING:
        return MISSING
    if isinstance(t, Refined):
        # A refinement can only be judged on the whole value.
        return decode_partial(t.base, v)
    if isinstance(t, Option):
        return None if v is None else decode_partial(t.inner, v)
    if isinstance(t, Prim):
        if t.name == "String":
            return str(v) if isinstance(v, str) else MISSING
        if t.name == "dynamic":
            return v
        if isinstance(v, (_OpenStr, _Open, _OpenList)):
            return MISSING
        if t.name == "Int":
            return int(v) if isinstance(v, (int, float)) and not isinstance(v, bool) and float(v).is_integer() else MISSING
        if t.name == "Float":
            return float(v) if isinstance(v, (int, float)) and not isinstance(v, bool) else MISSING
        if t.name == "Bool":
            return v if isinstance(v, bool) else MISSING
        return MISSING
    if isinstance(t, List):
        if not isinstance(v, list):
            return MISSING
        items = [decode_partial(t.elem, x) for x in v]
        return [x for x in items if x is not MISSING]
    if isinstance(t, Map):
        if not isinstance(v, dict):
            return MISSING
        out = {}
        for k, x in v.items():
            d = decode_partial(t.value, x)
            if d is not MISSING:
                out[k] = d
        return out
    if isinstance(t, Adt):
        return _decode_adt(t, v)
    return MISSING


def _decode_adt(t: Adt, v: Any) -> Any:
    from .schema import decode
    from .errors import DecodeError

    i = info(t.cls)
    if isinstance(i, RecordInfo):
        if not isinstance(v, dict):
            return MISSING
        fields: dict[str, Any] = {}
        for w, py, ft in i.fields:
            if w in v:
                d = decode_partial(subst(ft, t.args), v[w])
                if d is not MISSING:
                    fields[py] = d
        if not isinstance(v, _Open) and len(fields) == len(i.fields):
            try:
                return decode(t, _plain(v))
            except DecodeError:
                pass
        return Partial(t.cls, fields)
    # Enums: only a complete variant.
    if isinstance(v, (_OpenStr, _Open)):
        return MISSING
    try:
        return decode(t, _plain(v))
    except DecodeError:
        return MISSING


def _plain(v: Any) -> Any:
    if isinstance(v, dict):
        return {k: _plain(x) for k, x in v.items()}
    if isinstance(v, list):
        return [_plain(x) for x in v]
    if isinstance(v, str):
        return str(v)
    return v


__all__ = ["MISSING", "Partial", "PartialValue", "decode_partial", "parse_prefix", "some"]
