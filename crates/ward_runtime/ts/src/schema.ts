// Runtime descriptions of Wardscript types, and what they're for: JSON schemas for
// model answers, and decoding and encoding JSON values. Records are plain objects,
// enums without fields are strings (the variant's name), and variants with fields
// are `{tag, _0, _1, ...}` objects made by `variant()`.

import { DecodeError, WardError } from "./errors.ts";
import { Some, some, unwrap } from "./values.ts";

export abstract class Type {
  abstract toString(): string;
}

export class Prim extends Type {
  readonly name: string;
  constructor(name: string) {
    super();
    this.name = name;
  }
  toString(): string {
    return this.name;
  }
}

export const Int = new Prim("Int");
export const Float = new Prim("Float");
export const String = new Prim("String");
export const Bool = new Prim("Bool");
export const Unit = new Prim("()");
/** Values of tools without a schema. */
export const Any = new Prim("dynamic");

export class ListType extends Type {
  readonly elem: Type;
  constructor(elem: Type) {
    super();
    this.elem = elem;
  }
  toString(): string {
    return `List<${this.elem}>`;
  }
}

export class MapType extends Type {
  readonly key: Type;
  readonly value: Type;
  constructor(key: Type, value: Type) {
    super();
    this.key = key;
    this.value = value;
  }
  toString(): string {
    return `Map<${this.key}, ${this.value}>`;
  }
}

export class OptionType extends Type {
  readonly inner: Type;
  constructor(inner: Type) {
    super();
    this.inner = inner;
  }
  toString(): string {
    return `Option<${this.inner}>`;
  }
}

/** The `index`th generic parameter of the enclosing record or enum. */
export class ParamType extends Type {
  readonly index: number;
  constructor(index: number) {
    super();
    this.index = index;
  }
  toString(): string {
    return `T${this.index}`;
  }
}

export interface RecordInfo {
  kind: "record";
  name: string;
  /** `[name, type]`, in declaration order. */
  fields: () => [string, Type][];
}

export interface EnumInfo {
  kind: "enum";
  name: string;
  /** `[name, payload types]`. */
  variants: () => [string, Type[]][];
}

export type AdtInfo = RecordInfo | EnumInfo;

export function record(name: string, fields: () => [string, Type][]): RecordInfo {
  return { kind: "record", name, fields };
}

export function enum_(name: string, variants: () => [string, Type[]][]): EnumInfo {
  return { kind: "enum", name, variants };
}

export class AdtType extends Type {
  readonly info: AdtInfo;
  readonly args: Type[];
  constructor(info: AdtInfo, ...args: Type[]) {
    super();
    this.info = info;
    this.args = args;
  }
  toString(): string {
    return this.args.length ? `${this.info.name}<${this.args.join(", ")}>` : this.info.name;
  }
}

/** `base where cond`: checked while decoding; `schema` holds the JSON Schema keywords
 * the condition implies. */
export class RefinedType extends Type {
  readonly base: Type;
  readonly check: (value: never) => boolean;
  readonly text: string;
  readonly schema: Record<string, unknown>;
  constructor(base: Type, check: (value: never) => boolean, text: string, schema: Record<string, unknown> = {}) {
    super();
    this.base = base;
    this.check = check;
    this.text = text;
    this.schema = schema;
  }
  toString(): string {
    return `${this.base} where ${this.text}`;
  }
}

export const List = (elem: Type): Type => new ListType(elem);
export const Adt = (info: AdtInfo, ...args: Type[]): Type => new AdtType(info, ...args);
export const Param = (index: number): Type => new ParamType(index);
export const Refined = (
  base: Type,
  check: (value: never) => boolean,
  text: string,
  schema: Record<string, unknown> = {},
): Type => new RefinedType(base, check, text, schema);
export const Map_ = (key: Type, value: Type): Type => new MapType(key, value);
export const Option = (inner: Type): Type => new OptionType(inner);

/** Marks an object as an enum variant, so encoding tells it from a record. */
export const VARIANT: unique symbol = Symbol("wardscript.variant");

export function variant<T extends { tag: string }>(value: T): T {
  Object.defineProperty(value, VARIANT, { value: true, enumerable: false });
  return value;
}

export function isVariant(value: unknown): value is { tag: string } & Record<string, unknown> {
  return value !== null && typeof value === "object" && VARIANT in (value as object);
}

export function subst(t: Type, args: Type[]): Type {
  if (t instanceof ParamType) return args[t.index] ?? Any;
  if (t instanceof ListType) return new ListType(subst(t.elem, args));
  if (t instanceof MapType) return new MapType(subst(t.key, args), subst(t.value, args));
  if (t instanceof OptionType) return new OptionType(subst(t.inner, args));
  if (t instanceof AdtType) return new AdtType(t.info, ...t.args.map((a) => subst(a, args)));
  if (t instanceof RefinedType) return new RefinedType(subst(t.base, args), t.check, t.text, t.schema);
  return t;
}

// -- JSON Schema --

type Schema = Record<string, unknown>;

export function jsonSchema(t: Type): Schema {
  const defs: Record<string, Schema> = {};
  const root = schemaOf(t, defs);
  return Object.keys(defs).length ? { ...root, $defs: defs } : root;
}

function defName(t: AdtType): string {
  if (!t.args.length) return t.info.name;
  return [t.info.name, ...t.args.map((a) => `${a}`.replace(/</g, "_").replace(/>/g, "").replace(/, /g, "_"))].join("_");
}

function schemaOf(t: Type, defs: Record<string, Schema>): Schema {
  if (t === Int) return { type: "integer" };
  if (t === Float) return { type: "number" };
  if (t === String) return { type: "string" };
  if (t === Bool) return { type: "boolean" };
  if (t === Unit) return { type: "null" };
  if (t === Any) return {};
  if (t instanceof ListType) return { type: "array", items: schemaOf(t.elem, defs) };
  if (t instanceof MapType) return { type: "object", additionalProperties: schemaOf(t.value, defs) };
  if (t instanceof OptionType) return { anyOf: [schemaOf(t.inner, defs), { type: "null" }] };
  if (t instanceof RefinedType) {
    const base = schemaOf(t.base, defs);
    const rule = `must satisfy: ${t.text}`;
    const description = typeof base.description === "string" ? `${base.description} (${rule})` : rule;
    return { ...base, ...t.schema, description };
  }
  if (t instanceof AdtType) {
    const name = defName(t);
    if (!(name in defs)) {
      defs[name] = {};
      defs[name] = adtSchema(t, defs);
    }
    return { $ref: `#/$defs/${name}` };
  }
  throw new TypeError(`no JSON schema for ${t}`);
}

function adtSchema(t: AdtType, defs: Record<string, Schema>): Schema {
  const info = t.info;
  if (info.kind === "record") {
    const fields = info.fields();
    return {
      type: "object",
      properties: Object.fromEntries(fields.map(([n, ft]) => [n, schemaOf(subst(ft, t.args), defs)])),
      required: fields.map(([n]) => n),
      additionalProperties: false,
    };
  }
  const variants = info.variants();
  if (variants.every(([, fs]) => fs.length === 0)) {
    return { type: "string", enum: variants.map(([n]) => n) };
  }
  return {
    oneOf: variants.map(([n, fs]) => {
      if (!fs.length) return { const: n };
      const items = fs.map((ft) => schemaOf(subst(ft, t.args), defs));
      return {
        type: "object",
        properties: { [n]: { type: "array", prefixItems: items, minItems: items.length, maxItems: items.length } },
        required: [n],
        additionalProperties: false,
      };
    }),
  };
}

// -- decoding --

function kind(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return "a boolean";
  if (typeof value === "number") return "a number";
  if (typeof value === "string") return "a string";
  if (Array.isArray(value)) return "an array";
  if (typeof value === "object") return "an object";
  return typeof value;
}

/** Converts a JSON value to the JavaScript representation of `t`, or throws
 * `DecodeError` saying where it doesn't match. */
export function decode(t: Type, value: unknown, path = "$"): unknown {
  const fail = (expected: string) => new DecodeError(path, `expected ${expected}, found ${kind(value)}`);
  if (t === Any) return value;
  if (t === Int) {
    if (typeof value !== "number" || !Number.isInteger(value)) throw fail("an integer");
    return value;
  }
  if (t === Float) {
    if (typeof value !== "number") throw fail("a number");
    return value;
  }
  if (t === String) {
    if (typeof value !== "string") throw fail("a string");
    return value;
  }
  if (t === Bool) {
    if (typeof value !== "boolean") throw fail("a boolean");
    return value;
  }
  if (t === Unit) {
    if (value !== null && value !== undefined) throw fail("null");
    return null;
  }
  if (t instanceof ListType) {
    if (!Array.isArray(value)) throw fail("an array");
    return value.map((x, i) => decode(t.elem, x, `${path}[${i}]`));
  }
  if (t instanceof MapType) {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw fail("an object");
    const out = new Map<unknown, unknown>();
    for (const [k, v] of Object.entries(value as object)) {
      out.set(decodeKey(t.key, k, path), decode(t.value, v, `${path}.${k}`));
    }
    return out;
  }
  if (t instanceof OptionType) {
    return value === null || value === undefined ? null : some(decode(t.inner, value, path));
  }
  if (t instanceof RefinedType) {
    const decoded = decode(t.base, value, path);
    let ok: boolean;
    try {
      ok = (t.check as (v: unknown) => boolean)(decoded);
    } catch (e) {
      if (e instanceof WardError) throw new DecodeError(path, `checking \`${t.text}\` failed: ${e.message}`);
      throw e;
    }
    if (!ok) throw new DecodeError(path, `doesn't satisfy \`${t.text}\``);
    return decoded;
  }
  if (t instanceof AdtType) return decodeAdt(t, value, path, fail);
  throw new TypeError(`cannot decode ${t}`);
}

function decodeKey(t: Type, key: string, path: string): unknown {
  if (t === Int) {
    const n = Number(key);
    if (!Number.isInteger(n)) throw new DecodeError(path, `expected integer keys, found ${JSON.stringify(key)}`);
    return n;
  }
  return decode(t, key, path);
}

function decodeAdt(t: AdtType, value: unknown, path: string, fail: (e: string) => DecodeError): unknown {
  const info = t.info;
  if (info.kind === "record") {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw fail(`a \`${info.name}\` object`);
    const fields = info.fields();
    const names = new Set(fields.map(([n]) => n));
    for (const key of Object.keys(value as object)) {
      if (!names.has(key)) throw new DecodeError(path, `\`${info.name}\` has no field \`${key}\``);
    }
    const out: Record<string, unknown> = {};
    for (const [n, ft] of fields) {
      if (!(n in (value as object))) throw new DecodeError(path, `missing field \`${n}\` of \`${info.name}\``);
      out[n] = decode(subst(ft, t.args), (value as Record<string, unknown>)[n], `${path}.${n}`);
    }
    return out;
  }
  const variants = info.variants();
  const unitOnly = variants.every(([, fs]) => fs.length === 0);
  const names = variants.map(([n]) => `\`${n}\``).join(", ");
  if (typeof value === "string") {
    const v = variants.find(([n, fs]) => n === value && fs.length === 0);
    if (!v) throw new DecodeError(path, `\`${value}\` is not a variant of \`${info.name}\` without fields (variants: ${names})`);
    return unitOnly ? value : variant({ tag: value });
  }
  if (value !== null && typeof value === "object" && !Array.isArray(value) && !unitOnly) {
    const entries = Object.entries(value as object);
    if (entries.length === 1) {
      const [key, payload] = entries[0] as [string, unknown];
      const v = variants.find(([n, fs]) => n === key && fs.length > 0);
      if (v) {
        const fs = v[1];
        if (!Array.isArray(payload) || payload.length !== fs.length) {
          throw new DecodeError(`${path}.${key}`, `expected an array of ${fs.length} values`);
        }
        const out: Record<string, unknown> = { tag: key };
        fs.forEach((ft, n) => {
          out[`_${n}`] = decode(subst(ft, t.args), payload[n], `${path}.${key}[${n}]`);
        });
        return variant(out as { tag: string });
      }
      throw new DecodeError(path, `\`${key}\` is not a variant of \`${info.name}\` with fields (variants: ${names})`);
    }
  }
  throw fail(`a \`${info.name}\` variant`);
}

/** The JSON value for a Wardscript value; the inverse of `decode`. */
export function encode(value: unknown): unknown {
  if (value === null || value === undefined) return null;
  if (typeof value === "boolean" || typeof value === "number" || typeof value === "string") return value;
  if (value instanceof Some) return encode(unwrap(value));
  if (Array.isArray(value)) return value.map(encode);
  if (value instanceof Map) {
    return Object.fromEntries([...value].map(([k, v]) => [typeof k === "string" ? k : globalThis.String(encode(k)), encode(v)]));
  }
  if (isVariant(value)) {
    const payload = Object.keys(value).filter((k) => /^_\d+$/.test(k)).sort((a, b) => Number(a.slice(1)) - Number(b.slice(1)));
    return payload.length === 0 ? value.tag : { [value.tag]: payload.map((k) => encode(value[k])) };
  }
  if (typeof value === "object") {
    return Object.fromEntries(Object.entries(value as object).map(([k, v]) => [k, encode(v)]));
  }
  throw new TypeError(`cannot encode ${globalThis.String(value)}`);
}
