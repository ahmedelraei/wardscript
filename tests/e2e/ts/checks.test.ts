// ward: tests/e2e/checks.ward
// Refinements and checks, from TypeScript: a failed one is retried with the reason.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import * as checks from "./checks.ts";
import { AiOutputError, MockModel, Seq, configure, lastRun, reset } from "wardscript";

const GOOD = { subject: "Hello", body: "Hi Ada, thanks for writing." };

afterEach(() => reset());

const attempts = () =>
  (lastRun()?.records ?? []).filter((r) => r.kind === "ai_call").map((r) => [r.function, r.error]);

test("a failed check is retried with its reason", async () => {
  const model = new MockModel({ draft: new Seq({ ...GOOD, body: "Hi there" }, GOOD), is_polite: true });
  configure({ model });
  assert.deepEqual(await checks.reply("Ada"), GOOD);
  assert.deepEqual(attempts(), [
    ["draft", "the answer failed a check: greet the customer by name"],
    ["is_polite", null],
    ["draft", null],
  ]);
  assert.equal(model.calls.filter((c) => c.function === "draft")[1]?.errors[0], "the answer failed a check: greet the customer by name");
});

test("a refinement is checked while decoding", async () => {
  configure({ model: new MockModel({ draft: new Seq({ ...GOOD, subject: "x".repeat(40) }, GOOD), is_polite: true }) });
  await checks.reply("Ada");
  assert.deepEqual(attempts()[0], ["draft", "$.subject: doesn't satisfy `it.len() <= 30`"]);
});

test("a model judge fails the answer", async () => {
  configure({ retries: 1, model: new MockModel({ draft: GOOD, is_polite: false }) });
  await assert.rejects(checks.reply("Ada"), (e: unknown) => {
    assert.ok(e instanceof AiOutputError);
    assert.deepEqual(e.errors, ["the answer failed a check: be polite", "the answer failed a check: be polite"]);
    return true;
  });
});
