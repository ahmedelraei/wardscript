// ward: tests/e2e/fallback.ward
// Model policies, from TypeScript: retries, fallbacks, budgets for every attempt.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import * as fallback from "./fallback.ts";
import { BudgetExceeded, MockModel, ModelUnavailable, RateLimited, Seq, Usage, configure, lastRun, reset } from "wardscript";

afterEach(() => reset());

const attempts = () =>
  (lastRun()?.records ?? []).filter((r) => r.kind === "ai_call").map((r) => [r.model, r.error]);

test("a retry after a rate limit", async () => {
  configure({ models: { fast: new MockModel({ summarize: new Seq(new RateLimited("429"), "short") }) } });
  assert.equal(await fallback.digest("a long text"), "short");
  assert.deepEqual(attempts(), [["fast", "RateLimited: 429"], ["fast", null]]);
});

test("a fallback after a failing primary", async () => {
  configure({
    models: {
      fast: new MockModel({ summarize: new Seq(new ModelUnavailable("503"), new ModelUnavailable("503")) }),
      smart: new MockModel({ summarize: new Usage("from smart", 50) }),
    },
  });
  assert.equal(await fallback.digest("a long text"), "from smart");
  assert.deepEqual(attempts(), [["fast", "ModelUnavailable: 503"], ["fast", "ModelUnavailable: 503"], ["smart", null]]);
});

test("the budget counts failed attempts", async () => {
  configure({
    models: {
      fast: new MockModel({ summarize: new Seq(new RateLimited("429"), new RateLimited("429")) }),
      smart: new MockModel({ summarize: new Seq(new RateLimited("429"), "late") }),
    },
  });
  await assert.rejects(fallback.digest("a long text"), BudgetExceeded);
  assert.equal(attempts().length, 3);
});
