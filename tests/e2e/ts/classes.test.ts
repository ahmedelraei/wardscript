// ward: tests/e2e/classes.ward
// Objects from TypeScript: `Class$new(...)` creates one and awaits its `init`.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import * as c from "./classes.ts";
import { MockModel, Trusted, TrustError, configure, reset } from "wardscript";

afterEach(() => reset());

test("objects are shared", async () => {
  assert.equal(await c.shared(), 2);
});

test("virtual and super calls", async () => {
  const quiet = await c.Agent$new(new Trusted("quiet"));
  const loud = await c.Shouter$new(new Trusted("loud"), new Trusted(true));
  assert.ok(loud instanceof c.Agent);
  const out = await c.run_all(new Trusted([quiet, loud]), "hello");
  assert.deepEqual(out, ["quiet saw 1", "LOUD SAW 1"]);
  assert.equal(await loud.calls(), 1);
  assert.deepEqual(loud.notes, ["hello"]);
});

test("fields that only hold trusted data need vouching", async () => {
  await assert.rejects(c.Agent$new("unvouched"), TrustError);
});

test("an ai fn method sees the object", async () => {
  const model = new MockModel({ summarize: "two notes" });
  configure({ model });
  const a = await c.Agent$new(new Trusted("a"));
  await a.handle("first");
  await a.handle("second");
  assert.equal(await a.summarize(), "two notes");
  assert.match(model.calls[0].prompt, /first/);
});

test("a method that throws", async () => {
  const a = await c.Agent$new(new Trusted("a"));
  assert.equal(await c.guarded(a, 5), 0);
  await a.handle("x");
  await a.handle("y");
  assert.equal(await c.guarded(a, 1), -1);
});

test("interfaces and abstract classes", async () => {
  assert.equal(await c.roundtrip(), "store: kept");
  const m = await c.Memory$new();
  assert.equal(await c.remember(new Trusted(m), new Trusted("x")), "store: x");
  await assert.rejects(c.remember(new Trusted(m), "unvouched"), TrustError);
  assert.deepEqual(await c.names([m, m]), ["store", "store"]);
});
