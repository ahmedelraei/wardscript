// The operations generated code calls that depend on the configuration: model calls,
// approvals, checks and tools.

import * as audit from "./audit.ts";
import * as budget from "./budget.ts";
import { now } from "./core.ts";
import { type ApprovalRequest, config } from "./config.ts";
import {
  AiOutputError,
  ApprovalDenied,
  DecodeError,
  ModelError,
  NoModelError,
  Thrown,
  ToolError,
  TrustError,
} from "./errors.ts";
import { type AiRequest, type Completion, type Model, estimateTokens } from "./model.ts";
import { type Type, decode, encode, jsonSchema } from "./schema.ts";

class Attempt {
  readonly model: Model;
  readonly alias: string | null;
  readonly request: AiRequest;
  readonly number: number;
  readonly started = now();
  answer: [string, number, number | null] = ["", 0, null];

  constructor(fn: string, prompt: string, returns: Type, errors: string[], alias: string | null, number: number) {
    let model: Model | null | undefined;
    if (alias === null) {
      model = config.model;
      if (!model) throw new NoModelError(`\`${fn}\` needs a model; call configure({model: ...}) first`);
    } else {
      model = config.models[alias];
      if (!model) {
        const known = Object.keys(config.models).map((a) => `\`${a}\``).join(", ") || "none";
        throw new NoModelError(
          `\`${fn}\` asks for the model \`${alias}\`, but configure({models: ...}) doesn't have it (configured: ${known})`,
        );
      }
    }
    this.model = model;
    this.alias = alias;
    this.returns = returns;
    this.request = { function: fn, prompt, schema: jsonSchema(returns), attempt: errors.length, errors: [...errors] };
    this.number = number;
    const strict = config.unpriced === "error";
    budget.beforePricedCall(!("prices" in model) || model.prices !== null, strict);
    budget.beforeModelCall();
  }

  readonly returns: Type;

  decode(answer: string | Completion): [boolean, unknown, string | null] {
    const [text, tokens0, cost] =
      typeof answer === "string" ? [answer, null, null] : [answer.text, answer.tokens ?? null, answer.cost ?? null];
    const tokens = tokens0 ?? estimateTokens(this.request.prompt) + estimateTokens(text);
    const run = audit.current();
    if (run) {
      run.calls += 1;
      run.tokens += tokens;
      run.cost += cost ?? 0;
    }
    this.answer = [text, tokens, cost];
    try {
      return [true, decode(this.returns, JSON.parse(text)), null];
    } catch (e) {
      if (e instanceof SyntaxError) return [false, null, `the answer is not valid JSON (${e.message})`];
      if (e instanceof DecodeError) return [false, null, e.message];
      throw e;
    }
  }

  record(ok: boolean, value: unknown, error: string | null): void {
    const [text, tokens, cost] = this.answer;
    audit.record("ai_call", {
      started: this.started,
      function: this.request.function,
      attempt: this.number,
      model: this.alias,
      prompt: this.request.prompt,
      answer: text,
      tokens,
      cost,
      error,
      leaves: ok ? audit.leaves(value) : [],
    });
    budget.afterModelCall(tokens, cost, config.unpriced === "error");
  }

  failed(error: ModelError): void {
    const run = audit.current();
    if (run) run.calls += 1;
    audit.record("ai_call", {
      started: this.started,
      function: this.request.function,
      attempt: this.number,
      model: this.alias,
      prompt: this.request.prompt,
      answer: null,
      tokens: 0,
      cost: 0,
      error: `${error.name}: ${error.message}`,
      leaves: [],
    });
    budget.afterModelCall(0, 0);
  }
}

export interface AiOptions {
  /** Model aliases in order; `null` is the default model. */
  models?: (string | null)[];
  /** Retries of retryable provider errors. */
  retries?: number;
  /** Seconds before the first retry; it doubles each time. */
  backoff?: number;
  /** The `check {...}` clause: the first failed reason, or `null`. */
  check?: (value: never) => Promise<string | null> | string | null;
}

const sleep = (seconds: number) => new Promise((r) => setTimeout(r, seconds * 1000));

/** Calls the model for `ai fn fn` and decodes its answer as `returns`, retrying with
 * the error when it doesn't fit, retrying provider errors with backoff, and falling
 * back to the next model when one keeps failing. */
export async function ai(fn: string, prompt: string, returns: Type, options: AiOptions = {}): Promise<unknown> {
  const providerRetries = options.retries ?? config.modelRetries;
  const delay = options.backoff ?? config.backoff;
  const check = options.check as ((v: unknown) => Promise<string | null> | string | null) | undefined;
  let number = 0;
  let last: Error | null = null;
  for (const alias of options.models ?? [null]) {
    const errors: string[] = [];
    let failure: ModelError | null = null;
    for (let r = 0; r <= config.retries; r++) {
      failure = null;
      let answered = false;
      for (let n = 0; n <= providerRetries; n++) {
        const attempt = new Attempt(fn, prompt, returns, errors, alias, number++);
        let answer: string | Completion;
        try {
          answer = await attempt.model.complete(attempt.request);
        } catch (e) {
          if (!(e instanceof ModelError)) throw e;
          attempt.failed(e);
          failure = e;
          if (e.retryable && n < providerRetries) {
            budget.checkTime();
            await sleep(delay * 2 ** n);
            continue;
          }
          break;
        }
        answered = true;
        let [ok, value, error] = attempt.decode(answer);
        if (ok && check) {
          let reason: string | null;
          try {
            reason = await check(value);
          } catch (e) {
            attempt.record(false, null, "the checks failed to run");
            throw e;
          }
          if (reason !== null) [ok, error] = [false, `the answer failed a check: ${reason}`];
        }
        attempt.record(ok, value, error);
        if (ok) return value;
        errors.push(error ?? "");
        break;
      }
      if (failure !== null || !answered) break;
    }
    last = failure ?? new AiOutputError(fn, errors);
  }
  throw last ?? new AiOutputError(fn, []);
}

function validated(value: unknown, passed: boolean, ruleName: string, site: string): unknown {
  audit.record("validate", { rule: ruleName, site, passed, leaves: audit.leaves(value) });
  if (passed) return value;
  throw new Thrown(`validation failed: \`${ruleName}\` rejected the value`);
}

export async function validate<T>(
  value: T,
  rule: (v: T) => boolean | Promise<boolean>,
  ruleName: string,
  site = "?",
): Promise<T> {
  return validated(value, Boolean(await rule(value)), ruleName, site) as T;
}

export async function approve<T>(value: T, site: string): Promise<T> {
  const approver = config.approver;
  if (!approver) {
    throw new ApprovalDenied(`approval needed at ${site}, but no approver is configured; call configure({approver: ...})`);
  }
  const request: ApprovalRequest = { value, site, run: audit.current()?.id ?? null };
  const approved = Boolean(await approver(request));
  audit.record("approve", { site, approved, leaves: audit.leaves(value) });
  if (!approved) throw new ApprovalDenied(`approval denied at ${site}`);
  return value;
}

export function declassify<T>(value: T, reason: string, site = "?"): T {
  audit.record("declassify", { site, reason, leaves: audit.leaves(value) });
  return value;
}

export interface ToolSchema {
  mcpName?: string;
  names?: string[];
  sinks?: boolean[];
  returns?: Type;
}

/** A tool call. With a schema from `ward.lock`, an implementation with a
 * `callTool(name, arguments)` method (an MCP server) gets named arguments. */
export async function callTool(source: string, name: string, site: string, args: unknown[], schema: ToolSchema = {}): Promise<unknown> {
  budget.checkTime();
  const impl = config.tools[source] as Record<string, unknown> | undefined;
  if (!impl) {
    throw new ToolError(`tool \`${source}\` is not configured; call configure({tools: {${JSON.stringify(source)}: ...}})`);
  }
  let run: () => unknown;
  const named = (impl as { callTool?: unknown }).callTool;
  if (schema.names && typeof named === "function") {
    const argumentsObj: Record<string, unknown> = {};
    schema.names.forEach((n, i) => {
      if (i < args.length && args[i] !== null) argumentsObj[n] = encode(args[i]);
    });
    run = () => (named as (n: string, a: Record<string, unknown>) => unknown).call(impl, schema.mcpName ?? name, argumentsObj);
  } else {
    const f = impl[name];
    if (typeof f !== "function") throw new ToolError(`tool \`${source}\` has no function \`${name}\``);
    run = () => (f as (...a: unknown[]) => unknown).apply(impl, args);
  }
  const started = now();
  const done = (result: unknown, error: string | null) => {
    audit.record("tool_call", {
      started,
      tool: source,
      function: name,
      site,
      args: args.map(audit.toJson),
      digests: args.map(audit.digest),
      error,
      leaves: error === null ? audit.leaves(result) : [],
    });
    budget.checkTime();
  };
  try {
    args.forEach((a, i) => {
      if (!schema.sinks || (schema.sinks[i] ?? true)) audit.checkSink(`${source}.${name}`, i, a);
    });
  } catch (e) {
    if (e instanceof TrustError) done(null, `TrustError: ${e.message}`);
    throw e;
  }
  let result: unknown;
  try {
    result = await run();
    if (schema.returns) {
      try {
        result = decode(schema.returns, result, `result of \`${source}.${name}\``);
      } catch (e) {
        if (e instanceof DecodeError) {
          throw new ToolError(`\`${source}.${name}\` returned something its schema doesn't allow: ${e.message}`);
        }
        throw e;
      }
    }
  } catch (e) {
    done(null, e instanceof Error ? `${e.name}: ${e.message}` : String(e));
    throw e;
  }
  done(result, null);
  return result;
}
