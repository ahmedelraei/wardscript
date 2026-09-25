# WEP 020: Parsing numbers from strings

**Status:** Draft. Opened 2026-09-26.

## Summary

Two built-in methods, `String.parse_int() -> Option<Int>` and
`String.parse_float() -> Option<Float>`, so programs can compare numbers that tools
return as text.

## Motivation

Tools answer in text. AgentDojo's travel suite asks for "the best-rated hotel under
210", and the rating and price are in the listing as `rating: 4.7`. Without a
conversion, the only way to compare them is to ask a model, which turns a
deterministic choice into one an injected review can steer. The blind ports of WEP
018 listed "no string to number conversion" among their friction points.

```ward
fn cheaper(a: String, b: String) -> Bool {
    a.parse_float().unwrap_or(0.0) < b.parse_float().unwrap_or(0.0)
}
```

## Specification

- `parse_int()`: after trimming whitespace, the string must match `[+-]?[0-9]+` and
  fit in a 64-bit signed integer (on the TypeScript backend, a safe integer, the
  range `Int` is exact in). Otherwise `None`.
- `parse_float()`: after trimming, `[+-]?[0-9]+(\.[0-9]+)?`. Otherwise `None`. No
  exponents, `inf`, `nan`, leading or trailing `.`, or `_` separators, so both
  backends accept exactly the same strings.
- No effects.

## Trust and security

A method's result carries the labels of its receiver, as every built-in method
does: a number parsed from untrusted text is untrusted. Parsing is not validation;
to act on the number it must still go through `validate`, `approve` or
`declassify`. `tests/attacks/parse_int_amount.ward` charges an amount parsed from
an untrusted reply and is rejected with W0107.

Comparing parsed numbers inside a branch makes the branch depend on untrusted data,
so the usual implicit-flow rules apply to anything written there.

## Backwards compatibility

New method names on `String`; no existing program changes meaning.

## Rejected ideas

- **`Int.parse(s)` as a static function.** Wardscript has no static functions on
  primitive types; methods on the receiver match `to_float` and `round`.
- **Throwing `ParseError`.** `Option` is enough for "not a number" and composes with
  `unwrap_or`; a program that wants an error can `match`.
- **Accepting whatever the host language parses.** Python's `float()` takes `1e3`,
  `inf` and `1_000`, and JavaScript's `Number()` takes `0x10` and `""`. The same
  program would behave differently on each backend.

## Spec changes

`docs/spec/types.md` (built-in methods).
