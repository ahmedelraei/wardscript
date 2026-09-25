// Exceptions raised by Wardscript programs and by the runtime.

import { toStr } from "./values.ts";

/** A Wardscript `throw` that reached the host. The thrown value is `value`. */
export class Thrown extends Error {
  readonly value: unknown;
  constructor(value: unknown) {
    super(toStr(value));
    this.name = "Thrown";
    this.value = value;
  }
}

/** A runtime failure: something the program's types can't express, like a model that
 * keeps answering with invalid output. Never caught by Wardscript `try`. */
export class WardError extends Error {
  constructor(message: string) {
    super(message);
    this.name = new.target.name;
  }
}

export class NoModelError extends WardError {}

/** The model's answers didn't match the return type, on every attempt. */
export class AiOutputError extends WardError {
  readonly function: string;
  readonly errors: string[];
  constructor(fn: string, errors: string[]) {
    const attempts = errors.map((e, i) => `  attempt ${i + 1}: ${e}`).join("\n");
    super(`\`${fn}\` got no valid answer from the model:\n${attempts}`);
    this.function = fn;
    this.errors = errors;
  }
}

/** A model provider failed to answer. `retryable` ones are retried with backoff. */
export class ModelError extends WardError {
  readonly status: number | null;
  get retryable(): boolean {
    return false;
  }
  constructor(message: string, status: number | null = null) {
    super(message);
    this.status = status;
  }
}

/** The provider asked to slow down (HTTP 429). */
export class RateLimited extends ModelError {
  override get retryable(): boolean {
    return true;
  }
}

/** A timeout, a lost connection, or a server error (HTTP 5xx). */
export class ModelUnavailable extends ModelError {
  override get retryable(): boolean {
    return true;
  }
}

/** A function used more of a resource than its `budget` allows; the run stops. */
export class BudgetExceeded extends WardError {
  readonly function: string;
  readonly resource: string;
  readonly limit: number;
  readonly used: number;
  constructor(fn: string, resource: string, limit: number, used: number) {
    const show = (v: number) => (resource === "cost" ? `$${v}` : resource === "time" ? `${v}s` : `${v}`);
    super(`\`${fn}\` went over its ${resource} budget: used ${show(used)} of ${show(limit)}`);
    this.function = fn;
    this.resource = resource;
    this.limit = limit;
    this.used = used;
  }
}

/** A `cost` budget can't be enforced: the model's cost is unknown. */
export class BudgetUnenforceable extends WardError {
  readonly function: string;
  readonly when: "before" | "after";
  constructor(fn: string, when: "before" | "after") {
    const why =
      when === "before"
        ? "the model has no prices; give the provider `prices: [input, output]`"
        : "the model's answer didn't say what it cost";
    super(`\`${fn}\` has a cost budget, but ${why}`);
    this.function = fn;
    this.when = when;
  }
}

export class ApprovalDenied extends WardError {}
export class ToolError extends WardError {}

/** A JSON value doesn't match a Wardscript type. `path` locates it, e.g. `$.items[2]`. */
export class DecodeError extends WardError {
  readonly path: string;
  readonly detail: string;
  constructor(path: string, detail: string) {
    super(`${path}: ${detail}`);
    this.path = path;
    this.detail = detail;
  }
}

/** An operation with no defined result: an index out of bounds, division by zero. */
export class PanicError extends WardError {}

/** A host passed a value to a parameter that must be trusted without vouching for it. */
export class TrustError extends WardError {}

/** An `assert` in a `test` block didn't hold. */
export class TestFailure extends WardError {
  readonly site: string;
  constructor(message: string, site: string) {
    super(`assertion failed at ${site}: ${message}`);
    this.site = site;
  }
}
