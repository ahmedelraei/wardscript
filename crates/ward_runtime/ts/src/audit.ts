// The audit trace. A run starts when the host calls into Wardscript and ends when that
// call returns; the runtime records its model and tool calls and every `validate`,
// `approve` and `declassify`, one JSON line per event in `<traceDir>/<run>.jsonl`.

import { AsyncLocalStorage } from "node:async_hooks";

import * as core from "./core.ts";
import { Thrown, TrustError } from "./errors.ts";
import { encode } from "./schema.ts";
import { Trusted } from "./trust.ts";
import { config } from "./config.ts";

export class Run {
  readonly recorder: core.Recorder;
  readonly records: core.TraceRecord[] = [];
  tokens = 0;
  calls = 0;
  cost = 0;
  constructor(recorder: core.Recorder) {
    this.recorder = recorder;
  }
  get id(): string {
    return this.recorder.run;
  }
  get path(): string | null {
    return this.recorder.path;
  }
}

const storage = new AsyncLocalStorage<Run>();
let lastRun: Run | null = null;

export function current(): Run | undefined {
  return storage.getStore();
}

/** The most recent run that finished. */
export function last(): Run | null {
  return lastRun;
}

/** A JSON value for anything, for the trace. */
export function toJson(value: unknown): unknown {
  if (value instanceof Trusted) return toJson(value.value);
  try {
    return encode(value);
  } catch {
    return String(value);
  }
}

export function digest(value: unknown): string {
  return core.digest(toJson(value));
}

export function leaves(value: unknown): core.Leaf[] {
  return core.leaves(toJson(value));
}

export function record(kind: string, fields: Record<string, unknown>): void {
  const run = storage.getStore();
  if (!run) return;
  run.records.push(run.recorder.record(kind, fields));
}

// Leaves this short are too likely to match by chance.
const MIN_TAINT_LEN = 8;

function untrustedOrigin(run: Run, value: unknown): string | null {
  const sources = new Map<string, string>();
  const cleared = new Set<string>();
  const add = (ls: unknown, what: string) => {
    for (const l of ls as core.Leaf[]) if (!sources.has(l.digest)) sources.set(l.digest, what);
  };
  const clear = (ls: unknown) => {
    for (const l of ls as core.Leaf[]) cleared.add(l.digest);
  };
  for (const r of run.records) {
    switch (r.kind) {
      case "run_start":
        for (const a of r.args as { name: string; vouched: boolean; leaves: core.Leaf[] }[]) {
          if (a.vouched) clear(a.leaves);
          else add(a.leaves, `argument \`${a.name}\` from the host`);
        }
        break;
      case "ai_call":
        add(r.leaves, `the output of \`ai fn ${r.function}\``);
        break;
      case "tool_call":
        add(r.leaves, `the result of \`${r.tool}.${r.function}\``);
        break;
      case "validate":
        if (r.passed) clear(r.leaves);
        break;
      case "approve":
        if (r.approved) clear(r.leaves);
        break;
      case "declassify":
        clear(r.leaves);
        break;
    }
  }
  const walk = (v: unknown): string | null => {
    if (typeof v === "string" && [...v].length < MIN_TAINT_LEN) return null;
    if (v === null || typeof v === "boolean" || typeof v === "number") return null;
    if (Array.isArray(v) && v.length === 0) return null;
    if (typeof v === "object" && !Array.isArray(v) && Object.keys(v as object).length === 0) return null;
    const d = core.digest(v);
    if (cleared.has(d)) return null;
    const found = sources.get(d);
    if (found) return found;
    const parts = Array.isArray(v) ? v : typeof v === "object" ? Object.values(v as object) : [];
    for (const x of parts) {
      const f = walk(x);
      if (f) return f;
    }
    return null;
  };
  return walk(toJson(value));
}

/** Defense in depth behind the checker: refuses a tool argument that is, exactly, an
 * unchecked untrusted value. */
export function checkSink(tool: string, index: number, value: unknown): void {
  const run = storage.getStore();
  if (!run || !config.checkSinks) return;
  const origin = untrustedOrigin(run, value);
  if (origin !== null) {
    throw new TrustError(
      `argument ${index + 1} of \`${tool}\` is ${origin}, which no \`validate\`, \`approve\` ` +
        "or `declassify` checked; the tool was not called",
    );
  }
}

/** Entered by every generated function; the outermost one is a run. */
export async function call<T>(fn: string, args: [string, unknown][], body: () => Promise<T>): Promise<T> {
  if (storage.getStore()) return body();
  const run = new Run(new core.Recorder(config.traceDir ?? process.env.WARD_TRACE_DIR ?? null));
  return storage.run(run, async () => {
    record("run_start", {
      function: fn,
      args: args.map(([name, value]) => ({
        name,
        value: toJson(value),
        vouched: value instanceof Trusted,
        leaves: leaves(value),
      })),
    });
    let status = "ok";
    let error: string | null = null;
    try {
      return await body();
    } catch (e) {
      if (e instanceof Thrown) {
        status = "threw";
        error = JSON.stringify(toJson(e.value));
      } else {
        status = "error";
        error = e instanceof Error ? `${e.name}: ${e.message}` : String(e);
      }
      throw e;
    } finally {
      record("run_end", { status, error, tokens: run.tokens, calls: run.calls, cost: run.cost });
      lastRun = run;
    }
  });
}
