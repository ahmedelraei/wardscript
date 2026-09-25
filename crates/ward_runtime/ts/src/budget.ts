// Budget counters. A function declared with `budget {...}` runs inside `budget()`;
// every model call is charged to all the budgets active around it.

import { AsyncLocalStorage } from "node:async_hooks";

import { record } from "./audit.ts";
import { Budget, type Limits, type Over } from "./core.ts";
import { BudgetExceeded, BudgetUnenforceable } from "./errors.ts";

const active = new AsyncLocalStorage<Budget[]>();

export async function budget<T>(fn: string, limits: Limits, body: () => Promise<T>): Promise<T> {
  const b = new Budget(fn, limits);
  return active.run([...(active.getStore() ?? []), b], body);
}

function raise(b: Budget, over: Over): void {
  if (over) {
    const [resource, limit, used] = over;
    record("budget_exceeded", { function: b.function, resource, limit, used });
    throw new BudgetExceeded(b.function, resource, limit, used);
  }
}

export function checkTime(): void {
  for (const b of active.getStore() ?? []) raise(b, b.checkTime());
}

export function beforeModelCall(): void {
  for (const b of active.getStore() ?? []) raise(b, b.chargeCall());
}

const warned = new Set<string>();

function unenforceable(fn: string, when: "before" | "after", strict: boolean): void {
  if (strict) {
    record("budget_unenforceable", { function: fn, when });
    throw new BudgetUnenforceable(fn, when);
  }
  if (!warned.has(fn)) {
    warned.add(fn);
    process.emitWarning(new BudgetUnenforceable(fn, when).message);
  }
}

/** Refuses a request to a model without prices while a `cost` budget is active. */
export function beforePricedCall(priced: boolean, strict: boolean): void {
  if (priced) return;
  const b = (active.getStore() ?? []).find((b) => b.limitsCost);
  if (b) unenforceable(b.function, "before", strict);
}

export function afterModelCall(tokens: number, cost: number | null, strict = true): void {
  const bs = active.getStore() ?? [];
  const overs = bs.map((b) => [b, b.chargeUsage(tokens, cost)] as const);
  for (const [b, over] of overs) raise(b, over);
  const b = bs.find((b) => b.unenforceable(cost));
  if (b) unenforceable(b.function, "after", strict);
}
