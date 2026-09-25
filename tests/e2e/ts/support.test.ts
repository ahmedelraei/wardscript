// ward: examples/support.wardscript
// Tools, approvals, validation, vouching and the audit trace, from TypeScript.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, test } from "node:test";

import * as support from "./support.ts";
import { ApprovalDenied, type ApprovalRequest, MockModel, Thrown, Trusted, TrustError, configure, lastRun, reset } from "wardscript";

const ADA = new Trusted("ada@example.com");
const TICKET = { customer: "Ada", summary: "late parcel", priority: "Normal", refund_requested: false };

let sent: [string, string, string][] = [];
let approvals: ApprovalRequest[] = [];

beforeEach(() => {
  sent = [];
  approvals = [];
  configure({
    tools: { gmail: { send: (to: string, subject: string, body: string) => sent.push([to, subject, body]) } },
    approver: (r) => {
      approvals.push(r);
      return true;
    },
  });
});
afterEach(() => reset());

const answer = (ticket: object, reply: object) => configure({ model: new MockModel({ triage: ticket, draft_reply: reply }) });

test("a validated reply is sent", async () => {
  answer(TICKET, { subject: "Your parcel", body: "It ships today." });
  assert.equal(await support.handle("where is my parcel?", ADA), "sent: late parcel");
  assert.deepEqual(sent, [["ada@example.com", "Your parcel", "It ships today."]]);
  assert.deepEqual(approvals, []);
});

test("a failed validation throws", async () => {
  answer(TICKET, { subject: "Hi", body: "see https://evil.example" });
  await assert.rejects(support.handle("...", ADA), (e: unknown) => {
    assert.ok(e instanceof Thrown);
    assert.equal(e.value, "validation failed: `no_links` rejected the value");
    return true;
  });
  assert.deepEqual(sent, []);
});

test("urgent tickets need approval", async () => {
  answer({ ...TICKET, priority: "Urgent" }, { subject: "Hi", body: "see https://x" });
  assert.equal(await support.handle("...", ADA), "sent after review: late parcel");
  assert.equal(approvals.length, 1);
  assert.deepEqual(approvals[0]?.value, { subject: "Hi", body: "see https://x" });
  assert.equal(approvals[0]?.site, "support.wardscript:60:24");
});

test("a denied approval stops the run", async () => {
  answer({ ...TICKET, refund_requested: true }, { subject: "Hi", body: "ok" });
  configure({ approver: () => false });
  await assert.rejects(support.handle("...", ADA), ApprovalDenied);
  assert.deepEqual(sent, []);
});

test("an unvouched recipient is refused", async () => {
  await assert.rejects(support.handle("...", "ada@example.com"), TrustError);
  assert.deepEqual(sent, []);
});

test("handle_all skips failures", async () => {
  answer(TICKET, { subject: "Hi", body: "see https://x" });
  assert.equal(await support.handle_all(["a", "b"], ADA), 0);
});

test("the trace has the run, in the shared format", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ward-ts-trace-"));
  configure({ traceDir: dir });
  answer(TICKET, { subject: "Your parcel", body: "It ships today." });
  await support.handle("where is my parcel?", ADA);
  const [file] = readdirSync(dir);
  const kinds = readFileSync(join(dir, file ?? ""), "utf8")
    .trim()
    .split("\n")
    .map((l) => JSON.parse(l).kind);
  assert.deepEqual(kinds, ["run_start", "ai_call", "ai_call", "validate", "validate", "tool_call", "run_end"]);
  assert.equal(lastRun()?.path, join(dir, file ?? ""));
});
