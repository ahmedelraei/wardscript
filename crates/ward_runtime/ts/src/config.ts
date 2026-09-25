// The runtime's configuration; `configure` in `runtime.ts` sets it.

import type { Model } from "./model.ts";

export interface ApprovalRequest {
  value: unknown;
  /** Where `approve` was called, e.g. `support.ward:58:24`. */
  site: string;
  /** The run it's part of: `ward trace show <run>` shows how the value was made. */
  run: string | null;
}

export type Approver = (request: ApprovalRequest) => boolean | Promise<boolean>;

export interface Config {
  /** The model an `ai fn` asks when its `model {...}` clause names no `primary`. */
  model: Model | null;
  /** Models by the aliases `model {primary: fast}` uses. */
  models: Record<string, Model>;
  approver: Approver | null;
  /** Tool implementations by import source: `import mcp "gmail"` looks up `"gmail"`.
   * Each is an object with a function per tool function, or has `callTool(name, args)`. */
  tools: Record<string, unknown>;
  /** Retries after an invalid answer. */
  retries: number;
  /** Retries of a request that failed with a retryable `ModelError`. */
  modelRetries: number;
  /** Seconds before the first such retry; each next one waits twice as long. */
  backoff: number;
  /** Where each run's audit trace is written; else `WARD_TRACE_DIR`, else nowhere. */
  traceDir: string | null;
  /** Refuse a tool call whose argument is exactly an unchecked untrusted value. */
  checkSinks: boolean;
  /** `"error"` fails a `cost` budget with a model of unknown cost; `"warn"` warns. */
  unpriced: "error" | "warn";
}

function defaults(): Config {
  return {
    model: null,
    models: {},
    approver: null,
    tools: {},
    retries: 2,
    modelRetries: 2,
    backoff: 1,
    traceDir: null,
    checkSinks: true,
    unpriced: "error",
  };
}

export const config: Config = defaults();

export function configure(settings: Partial<Config>): void {
  for (const [k, v] of Object.entries(settings)) {
    if (v === undefined) continue;
    if ((k === "retries" || k === "modelRetries" || k === "backoff") && (v as number) < 0) {
      throw new RangeError(`${k} must be at least 0`);
    }
    if (k === "unpriced" && v !== "error" && v !== "warn") {
      throw new RangeError('unpriced must be "error" or "warn"');
    }
    (config as unknown as Record<string, unknown>)[k] = v;
  }
}

/** Restores the default configuration. */
export function reset(): void {
  Object.assign(config, defaults());
}
