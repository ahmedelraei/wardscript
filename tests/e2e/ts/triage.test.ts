// ward: examples/triage.ward
// A mock model's answer comes back as a typed `Ticket`, from TypeScript.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import * as triage from "./triage.ts";
import { AiOutputError, MockModel, NoModelError, Raw, Seq, configure, reset } from "wardscript";

const ANSWER = {
  customer: "Ada",
  summary: "Checkout crashes",
  priority: "Urgent",
  category: { Bug: ["checkout"] },
  tags: ["crash", "web"],
  order_id: 1042,
};

afterEach(() => reset());

test("routes with a typed ticket", async () => {
  const model = new MockModel({ triage: ANSWER });
  configure({ model });
  assert.equal(await triage.route("My checkout crashed on order 1042"), "[urgent] engineering/checkout: Checkout crashes (order 1042)");
  const [request] = model.calls;
  assert.equal(request?.function, "triage");
  assert.match(request?.prompt ?? "", /Email:\nMy checkout crashed on order 1042/);
  const defs = request?.schema.$defs as Record<string, { required?: string[]; enum?: string[] }>;
  assert.deepEqual(defs.Ticket?.required, Object.keys(ANSWER));
  assert.deepEqual(defs.Priority?.enum, ["Low", "Normal", "Urgent"]);
});

test("invalid answers are retried", async () => {
  const model = new MockModel({ triage: new Seq(new Raw("not json"), { ...ANSWER, priority: "Soon" }, { ...ANSWER, order_id: null, priority: "Low" }) });
  configure({ model });
  assert.equal(await triage.route("..."), "engineering/checkout: Checkout crashes");
  assert.deepEqual(model.calls.map((r) => r.attempt), [0, 1, 2]);
  assert.match(model.calls[1]?.errors[0] ?? "", /not valid JSON/);
  assert.match(model.calls[2]?.errors[1] ?? "", /\$\.priority/);
});

test("gives up after the retries", async () => {
  configure({ model: new MockModel({ triage: { ...ANSWER, tags: "crash" } }), retries: 1 });
  await assert.rejects(triage.route("..."), (e: unknown) => {
    assert.ok(e instanceof AiOutputError);
    assert.equal(e.errors.length, 2);
    assert.match(e.errors[0] ?? "", /\$\.tags: expected an array/);
    return true;
  });
});

test("needs a model", async () => {
  await assert.rejects(triage.route("..."), NoModelError);
});
