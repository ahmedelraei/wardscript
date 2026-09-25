// The runtime core, as in `ward_runtime` (Rust) and `wardscript._core_py` (Python):
// budget counters, digests of values, and the audit trace's records, written as JSON
// Lines that `ward trace show` reads.

import { appendFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

// -- digests --

function canonical(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null";
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  const keys = Object.keys(value).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${canonical((value as Record<string, unknown>)[k])}`).join(",")}}`;
}

/** FNV-1a 64 of the value's canonical JSON. */
export function digest(value: unknown): string {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(canonical(value))) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

const MAX_LEAVES = 512;

export interface Leaf {
  path: string;
  digest: string;
}

/** Digests of the value and every part of it, parents first. */
export function leaves(value: unknown): Leaf[] {
  const out: Leaf[] = [];
  const walk = (v: unknown, path: string) => {
    if (out.length >= MAX_LEAVES) return;
    out.push({ path, digest: digest(v) });
    if (Array.isArray(v)) v.forEach((x, i) => walk(x, `${path}[${i}]`));
    else if (v !== null && typeof v === "object") {
      for (const k of Object.keys(v).sort()) walk((v as Record<string, unknown>)[k], `${path}.${k}`);
    }
  };
  walk(value, "$");
  return out;
}

// -- budgets --

export type Resource = "tokens" | "calls" | "cost" | "time";
export type Limits = Partial<Record<Resource, number>>;
/** `[resource, limit, used]` of the first resource over its limit. */
export type Over = [Resource, number, number] | null;

export class Budget {
  readonly function: string;
  readonly limits: Limits;
  readonly used = { tokens: 0, calls: 0, cost: 0 };
  private readonly started = performance.now();

  constructor(fn: string, limits: Limits) {
    this.function = fn;
    this.limits = limits;
  }

  private over(resource: Resource, used: number): Over {
    const limit = this.limits[resource];
    return limit !== undefined && used > limit ? [resource, limit, used] : null;
  }

  private check(): Over {
    return (
      this.over("tokens", this.used.tokens) ??
      this.over("calls", this.used.calls) ??
      this.over("cost", this.used.cost) ??
      this.checkTime()
    );
  }

  get limitsCost(): boolean {
    return this.limits.cost !== undefined;
  }

  chargeCall(): Over {
    this.used.calls += 1;
    return this.check();
  }

  /** `cost` is `null` when unknown: it counts as 0, and `unenforceable` says whether
   * that may go on. */
  chargeUsage(tokens: number, cost: number | null): Over {
    this.used.tokens += tokens;
    this.used.cost += cost ?? 0;
    return this.check();
  }

  unenforceable(cost: number | null): boolean {
    return cost === null && this.limitsCost;
  }

  checkTime(): Over {
    return this.over("time", (performance.now() - this.started) / 1000);
  }
}

// -- the trace --

/** Unix time in nanoseconds. */
export function now(): bigint {
  return BigInt(Date.now()) * 1_000_000n + (process.hrtime.bigint() % 1_000_000n);
}

function newRunId(): string {
  const nanos = now();
  const mix = Number((nanos ^ BigInt(process.pid * 2654435761)) & 0xffffn);
  return (nanos / 1_000_000n).toString(16).padStart(11, "0") + mix.toString(16).padStart(4, "0");
}

// Field order of each event, as the Rust core writes it.
const EVENTS: Record<string, string[]> = {
  run_start: ["function", "args"],
  run_end: ["status", "error", "tokens", "calls", "cost"],
  ai_call: ["started", "function", "attempt", "model", "prompt", "answer", "tokens", "cost", "error", "leaves"],
  tool_call: ["started", "tool", "function", "site", "args", "digests", "error", "leaves"],
  validate: ["rule", "site", "passed", "leaves"],
  approve: ["site", "approved", "leaves"],
  declassify: ["site", "reason", "leaves"],
  budget_exceeded: ["function", "resource", "limit", "used"],
  budget_unenforceable: ["function", "when"],
};

export type TraceRecord = Record<string, unknown>;

export class Recorder {
  readonly run = newRunId();
  readonly path: string | null;
  private seq = 0;

  constructor(dir: string | null) {
    if (dir) {
      mkdirSync(dir, { recursive: true });
      this.path = join(dir, `${this.run}.jsonl`);
    } else {
      this.path = null;
    }
  }

  /** Records an event and returns it as a record (times as strings of nanoseconds). */
  record(kind: string, fields: Record<string, unknown>): TraceRecord {
    const order = EVENTS[kind];
    if (!order) throw new Error(`unknown event kind ${kind}`);
    const time = now();
    const parts = [
      `"run":${JSON.stringify(this.run)}`,
      `"seq":${this.seq}`,
      `"time":${time}`,
      `"kind":${JSON.stringify(kind)}`,
    ];
    const rec: TraceRecord = { run: this.run, seq: this.seq, time: time.toString(), kind };
    for (const name of order) {
      if (!(name in fields)) throw new Error(`missing field \`${name}\``);
      const v = fields[name];
      // `started` is nanoseconds, like `time`.
      parts.push(`${JSON.stringify(name)}:${typeof v === "bigint" ? v.toString() : JSON.stringify(v ?? null)}`);
      rec[name] = typeof v === "bigint" ? v.toString() : v;
    }
    this.seq += 1;
    if (this.path) appendFileSync(this.path, `{${parts.join(",")}}\n`);
    return rec;
  }
}
