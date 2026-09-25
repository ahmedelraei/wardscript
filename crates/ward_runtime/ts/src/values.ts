// How Wardscript values are represented in JavaScript, and the operations generated
// code needs. `Option<T>` is `null` or the value itself; the one value that can't be
// represented that way, `Some(x)` where `x` is `null` or a `Some`, is wrapped in
// `Some`. Unit is `null`. Lists are arrays and maps are `Map`s, never mutated.

import { PanicError } from "./errors.ts";

export class Some<T = unknown> {
  readonly value: T;
  constructor(value: T) {
    this.value = value;
  }
}

/** The type of an `Option<T>`. */
export type Option<T> = Exclude<T, null | undefined> | Some<T> | null;

export function some<T>(value: T): Option<T> {
  return (value === null || value === undefined || value instanceof Some ? new Some(value) : value) as Option<T>;
}

export function unwrap<T>(option: Option<T>): T {
  return (option instanceof Some ? option.value : option) as T;
}

export function unwrapOr<T>(option: Option<T>, fallback: T): T {
  return option === null ? fallback : unwrap(option);
}

/** Integer division, rounding towards zero. */
export function idiv(a: number, b: number): number {
  if (b === 0) throw new PanicError("division by zero");
  return Math.trunc(a / b);
}

/** Remainder with the sign of `a`, so that `a == (a / b) * b + a % b`. */
export function irem(a: number, b: number): number {
  return a - b * idiv(a, b);
}

export function fdiv(a: number, b: number): number {
  return a / b;
}

export function frem(a: number, b: number): number {
  return b === 0 ? NaN : a % b;
}

export function roundHalfAway(x: number): number {
  if (!Number.isFinite(x)) throw new PanicError(`cannot round ${x} to an integer`);
  return x >= 0 ? Math.floor(x + 0.5) : -Math.floor(-x + 0.5);
}

function checkIndex(xs: readonly unknown[], i: number): void {
  if (!(i >= 0 && i < xs.length)) {
    throw new PanicError(`index ${i} is out of bounds for a list of length ${xs.length}`);
  }
}

export function index<T>(xs: readonly T[], i: number): T {
  checkIndex(xs, i);
  return xs[i] as T;
}

export function listGet<T>(xs: readonly T[], i: number): Option<T> {
  return i >= 0 && i < xs.length ? some(xs[i] as T) : null;
}

export function first<T>(xs: readonly T[]): Option<T> {
  return xs.length > 0 ? some(xs[0] as T) : null;
}

export function last<T>(xs: readonly T[]): Option<T> {
  return xs.length > 0 ? some(xs[xs.length - 1] as T) : null;
}

export function listSet<T>(xs: readonly T[], i: number, value: T): T[] {
  checkIndex(xs, i);
  return [...xs.slice(0, i), value, ...xs.slice(i + 1)];
}

export function mapGet<K, V>(m: ReadonlyMap<K, V>, key: K): Option<V> {
  return m.has(key) ? some(m.get(key) as V) : null;
}

export function mapIndex<K, V>(m: ReadonlyMap<K, V>, key: K): V {
  if (!m.has(key)) throw new PanicError(`key ${toStr(key)} is not in the map`);
  return m.get(key) as V;
}

export function mapSet<K, V>(m: ReadonlyMap<K, V>, key: K, value: V): Map<K, V> {
  const out = new Map(m);
  out.set(key, value);
  return out;
}

/** Characters, as Python and Rust count them (not UTF-16 units). */
export function strLen(s: string): number {
  let n = 0;
  for (const _ of s) n++;
  return n;
}

export function lines(s: string): string[] {
  const out = s.split(/\r\n|\n|\r/);
  if (out.length > 0 && out[out.length - 1] === "") out.pop();
  return out;
}

/** A field of a tool result. */
export function field(obj: unknown, name: string): unknown {
  if (obj instanceof Map) return obj.get(name);
  return (obj as Record<string, unknown>)[name];
}

export function withField(obj: unknown, name: string, value: unknown): unknown {
  if (obj instanceof Map) return mapSet(obj, name, value);
  return { ...(obj as object), [name]: value };
}

/** A float as Python and Rust print it: `1.0`, `0.5`. */
export function floatStr(x: number): string {
  if (Number.isInteger(x) && Math.abs(x) < 1e16) return x.toFixed(1);
  if (Number.isNaN(x)) return "nan";
  if (!Number.isFinite(x)) return x > 0 ? "inf" : "-inf";
  return String(x);
}

/** Text for string templates and `to_string()`. */
export function toStr(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return String(value);
  if (value instanceof Some) return toStr(value.value);
  if (value === null || value === undefined) return "None";
  return JSON.stringify(plainJson(value));
}

function plainJson(value: unknown): unknown {
  if (value instanceof Some) return plainJson(value.value);
  if (value instanceof Map) return Object.fromEntries([...value].map(([k, v]) => [String(k), plainJson(v)]));
  if (Array.isArray(value)) return value.map(plainJson);
  if (value !== null && typeof value === "object") {
    const o = value as Record<string, unknown>;
    if (typeof o.tag === "string") {
      const payload = Object.keys(o).filter((k) => /^_\d+$/.test(k));
      return payload.length === 0 ? o.tag : { [o.tag]: payload.map((k) => plainJson(o[k])) };
    }
    return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, plainJson(v)]));
  }
  return value;
}

/** Structural equality, as Wardscript's `==`. */
export function eq(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a === "number" && typeof b === "number") return a === b;
  if (a instanceof Some && b instanceof Some) return eq(a.value, b.value);
  if (a instanceof Map && b instanceof Map) {
    if (a.size !== b.size) return false;
    for (const [k, v] of a) if (!b.has(k) || !eq(v, b.get(k))) return false;
    return true;
  }
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((x, i) => eq(x, b[i]));
  }
  if (a !== null && b !== null && typeof a === "object" && typeof b === "object") {
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    return (
      ka.length === kb.length &&
      ka.every((k) => eq((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]))
    );
  }
  return false;
}
