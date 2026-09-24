"""Runtime descriptions of Wardscript types, and what they're for: JSON schemas for
model answers, and decoding and encoding JSON values.

Generated modules describe their records and enums with `record()` and `enum()`, and
refer to types with the descriptors below (`List(String)`, `Adt(Ticket)`, ...).
"""

from __future__ import annotations

import enum as _enum
from dataclasses import dataclass
from typing import Any as _Any, Callable

from .errors import DecodeError
from .values import Some, some, unwrap


class Type:
    """A Wardscript type."""


@dataclass(frozen=True)
class Prim(Type):
    name: str

    def __repr__(self) -> str:
        return self.name


Int = Prim("Int")
Float = Prim("Float")
String = Prim("String")
Bool = Prim("Bool")
Unit = Prim("()")
#: Tool results, whose types aren't known yet.
Any = Prim("dynamic")


@dataclass(frozen=True)
class List(Type):
    elem: Type

    def __repr__(self) -> str:
        return f"List<{self.elem!r}>"


@dataclass(frozen=True)
class Map(Type):
    key: Type
    value: Type

    def __repr__(self) -> str:
        return f"Map<{self.key!r}, {self.value!r}>"


@dataclass(frozen=True)
class Option(Type):
    inner: Type

    def __repr__(self) -> str:
        return f"Option<{self.inner!r}>"


@dataclass(frozen=True)
class Param(Type):
    """The `index`th generic parameter of the enclosing record or enum."""

    index: int


class Adt(Type):
    """A record or enum, with its type arguments."""

    def __init__(self, cls: type, *args: Type) -> None:
        self.cls = cls
        self.args = args

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Adt) and (self.cls, self.args) == (other.cls, other.args)

    def __hash__(self) -> int:
        return hash((self.cls, self.args))

    def __repr__(self) -> str:
        name = info(self.cls).name
        return f"{name}<{', '.join(map(repr, self.args))}>" if self.args else name


@dataclass
class RecordInfo:
    name: str
    #: `(wardscript name, python attribute, type)`, in declaration order.
    fields: list[tuple[str, str, Type]]


@dataclass
class VariantInfo:
    name: str
    #: The `enum.Enum` member, or the variant's class for enums with payloads.
    value: _Any
    fields: list[Type]


@dataclass
class EnumInfo:
    name: str
    variants: list[VariantInfo]

    @property
    def unit_only(self) -> bool:
        return all(not v.fields for v in self.variants)


# Field lists are thunks: types may refer to classes defined later, or in modules
# that import this one.
def record(cls: type, name: str, fields: Callable[[], list[tuple[str, str, Type]]]) -> None:
    cls.__ward__ = (RecordInfo, name, fields)  # type: ignore[attr-defined]


def enum(cls: type, name: str, variants: Callable[[], list[tuple[str, _Any, list[Type]]]]) -> None:
    cls.__ward__ = (EnumInfo, name, variants)  # type: ignore[attr-defined]


def info(cls: type) -> RecordInfo | EnumInfo:
    raw = cls.__dict__.get("__ward__")
    if raw is None:
        # A variant class of an enum with payloads: its base carries the info.
        for base in cls.__mro__[1:]:
            if "__ward__" in base.__dict__:
                return info(base)
        raise TypeError(f"{cls.__name__} is not a Wardscript type")
    if isinstance(raw, tuple):
        kind, name, thunk = raw
        if kind is RecordInfo:
            raw = RecordInfo(name, list(thunk()))
        else:
            raw = EnumInfo(name, [VariantInfo(n, v, list(fs)) for n, v, fs in thunk()])
        cls.__ward__ = raw  # type: ignore[attr-defined]
    return raw


def subst(t: Type, args: tuple[Type, ...]) -> Type:
    if isinstance(t, Param):
        return args[t.index]
    if isinstance(t, List):
        return List(subst(t.elem, args))
    if isinstance(t, Map):
        return Map(subst(t.key, args), subst(t.value, args))
    if isinstance(t, Option):
        return Option(subst(t.inner, args))
    if isinstance(t, Adt):
        return Adt(t.cls, *(subst(a, args) for a in t.args))
    return t


_PRIM_SCHEMAS = {
    Int: {"type": "integer"},
    Float: {"type": "number"},
    String: {"type": "string"},
    Bool: {"type": "boolean"},
    Unit: {"type": "null"},
    Any: {},
}


def json_schema(t: Type) -> dict:
    """A JSON Schema (2020-12) for `t`. Records and enums go in `$defs`."""
    defs: dict[str, dict] = {}
    root = _schema(t, defs)
    return {**root, "$defs": defs} if defs else root


def _def_name(t: Adt) -> str:
    name = info(t.cls).name
    if not t.args:
        return name
    return "_".join([name, *(repr(a).replace("<", "_").replace(">", "").replace(", ", "_") for a in t.args)])


def _schema(t: Type, defs: dict[str, dict]) -> dict:
    if isinstance(t, Prim):
        return dict(_PRIM_SCHEMAS[t])
    if isinstance(t, List):
        return {"type": "array", "items": _schema(t.elem, defs)}
    if isinstance(t, Map):
        return {"type": "object", "additionalProperties": _schema(t.value, defs)}
    if isinstance(t, Option):
        return {"anyOf": [_schema(t.inner, defs), {"type": "null"}]}
    if isinstance(t, Adt):
        name = _def_name(t)
        if name not in defs:
            defs[name] = {}  # placeholder, so recursive types terminate
            defs[name] = _adt_schema(t, defs)
        return {"$ref": f"#/$defs/{name}"}
    raise TypeError(f"no JSON schema for {t!r}")


def _adt_schema(t: Adt, defs: dict[str, dict]) -> dict:
    i = info(t.cls)
    if isinstance(i, RecordInfo):
        return {
            "type": "object",
            "properties": {w: _schema(subst(ft, t.args), defs) for w, _, ft in i.fields},
            "required": [w for w, _, _ in i.fields],
            "additionalProperties": False,
        }
    if i.unit_only:
        return {"type": "string", "enum": [v.name for v in i.variants]}
    options: list[dict] = []
    for v in i.variants:
        if not v.fields:
            options.append({"const": v.name})
            continue
        items = [_schema(subst(ft, t.args), defs) for ft in v.fields]
        payload = {"type": "array", "prefixItems": items, "minItems": len(items), "maxItems": len(items)}
        options.append(
            {
                "type": "object",
                "properties": {v.name: payload},
                "required": [v.name],
                "additionalProperties": False,
            }
        )
    return {"oneOf": options}


def _kind(value: _Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "a boolean"
    if isinstance(value, (int, float)):
        return "a number"
    if isinstance(value, str):
        return "a string"
    if isinstance(value, list):
        return "an array"
    if isinstance(value, dict):
        return "an object"
    return type(value).__name__


def decode(t: Type, value: _Any, path: str = "$") -> _Any:
    """Converts a JSON value to the Python representation of `t`, or raises
    `DecodeError` saying where it doesn't match."""

    def fail(expected: str) -> DecodeError:
        return DecodeError(path, f"expected {expected}, found {_kind(value)}")

    if t is Any:
        return value
    if t is Int:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise fail("an integer")
        if isinstance(value, float) and not value.is_integer():
            raise fail("an integer")
        return int(value)
    if t is Float:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise fail("a number")
        return float(value)
    if t is String:
        if not isinstance(value, str):
            raise fail("a string")
        return value
    if t is Bool:
        if not isinstance(value, bool):
            raise fail("a boolean")
        return value
    if t is Unit:
        if value is not None:
            raise fail("null")
        return None
    if isinstance(t, List):
        if not isinstance(value, list):
            raise fail("an array")
        return [decode(t.elem, x, f"{path}[{i}]") for i, x in enumerate(value)]
    if isinstance(t, Map):
        if not isinstance(value, dict):
            raise fail("an object")
        return {
            _decode_key(t.key, k, path): decode(t.value, v, f"{path}.{k}") for k, v in value.items()
        }
    if isinstance(t, Option):
        return None if value is None else some(decode(t.inner, value, path))
    if isinstance(t, Adt):
        return _decode_adt(t, value, path, fail)
    raise TypeError(f"cannot decode {t!r}")


def _decode_key(t: Type, key: str, path: str) -> _Any:
    if t is Int:
        try:
            return int(key)
        except ValueError:
            raise DecodeError(path, f"expected integer keys, found {key!r}") from None
    return decode(t, key, path)


def _decode_adt(t: Adt, value: _Any, path: str, fail: Callable[[str], DecodeError]) -> _Any:
    i = info(t.cls)
    if isinstance(i, RecordInfo):
        if not isinstance(value, dict):
            raise fail(f"a `{i.name}` object")
        names = {w for w, _, _ in i.fields}
        for key in value:
            if key not in names:
                raise DecodeError(path, f"`{i.name}` has no field `{key}`")
        kwargs = {}
        for w, py, ft in i.fields:
            if w not in value:
                raise DecodeError(path, f"missing field `{w}` of `{i.name}`")
            kwargs[py] = decode(subst(ft, t.args), value[w], f"{path}.{w}")
        return t.cls(**kwargs)

    names = ", ".join(f"`{v.name}`" for v in i.variants)
    if isinstance(value, str):
        for v in i.variants:
            if v.name == value and not v.fields:
                return v.value if i.unit_only else v.value()
        raise DecodeError(path, f"`{value}` is not a variant of `{i.name}` without fields (variants: {names})")
    if isinstance(value, dict) and len(value) == 1 and not i.unit_only:
        [(key, payload)] = value.items()
        for v in i.variants:
            if v.name == key and v.fields:
                if not isinstance(payload, list) or len(payload) != len(v.fields):
                    raise DecodeError(f"{path}.{key}", f"expected an array of {len(v.fields)} values")
                return v.value(
                    *(decode(subst(ft, t.args), x, f"{path}.{key}[{n}]") for n, (ft, x) in enumerate(zip(v.fields, payload)))
                )
        raise DecodeError(path, f"`{key}` is not a variant of `{i.name}` with fields (variants: {names})")
    raise fail(f"a `{i.name}` variant")


def encode(value: _Any) -> _Any:
    """The JSON value for a Wardscript value; the inverse of `decode`."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, Some):
        return encode(unwrap(value))
    if isinstance(value, list):
        return [encode(x) for x in value]
    if isinstance(value, dict):
        return {k if isinstance(k, str) else str(encode(k)): encode(v) for k, v in value.items()}
    if isinstance(value, _enum.Enum):
        return value.name
    i = info(type(value))
    if isinstance(i, RecordInfo):
        return {w: encode(getattr(value, py)) for w, py, _ in i.fields}
    for v in i.variants:
        if type(value) is v.value:
            if not v.fields:
                return v.name
            return {v.name: [encode(getattr(value, f"_{n}")) for n in range(len(v.fields))]}
    raise TypeError(f"cannot encode {value!r}")
