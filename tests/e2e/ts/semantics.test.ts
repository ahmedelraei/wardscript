// ward: tests/e2e/semantics.ward
// The TypeScript backend agrees with the Python one on the language's semantics.
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";

import * as s from "./semantics.ts";
import { PanicError, Thrown, configure, reset } from "wardscript";

afterEach(() => reset());

test("integer division truncates", async () => {
  assert.equal(await s.int_div(7, 2), 3);
  assert.equal(await s.int_div(-7, 2), -3);
  assert.equal(await s.int_rem(-7, 2), -1);
  assert.equal(await s.int_rem(7, -2), 1);
  await assert.rejects(s.int_div(1, 0), PanicError);
});

test("float remainder and rounding", async () => {
  assert.equal(await s.float_rem(-7.5, 2.0), -1.5);
  assert.equal(await s.rounded(2.5), 3);
  assert.equal(await s.rounded(-2.5), -3);
  assert.equal(await s.float_div(1.0, 0.0), Infinity);
  assert.equal(await s.float_div(3.0, 2.0), 1.5);
});

test("names", async () => {
  assert.equal(await s.shadowing(), 12);
  assert.equal(await s.keywords(2), 5);
});

test("bools print like Wardscript", async () => {
  assert.equal(await s.bools(true, false), "true false false");
});

test("comparisons don't chain", async () => {
  assert.equal(await s.compare(1, 1, true), true);
  assert.equal(await s.compare(1, 2, true), false);
});

test("assignment copies", async () => {
  const p: s.Point = { x: 1, from: 2 };
  assert.deepEqual(await s.moved(p), { x: 100, from: 2 });
  assert.equal(p.x, 1);
  const boxes: s.Box<s.Point>[] = [{ item: { x: 0, from: 0 } }, { item: { x: 0, from: 0 } }];
  const out = await s.set_nested(boxes, 1, 7);
  assert.equal(out[1]?.item.x, 7);
  assert.equal(boxes[1]?.item.x, 0);
  await assert.rejects(s.set_nested(boxes, 5, 7), PanicError);
  const m = new Map([["a", 1]]);
  assert.deepEqual(await s.set_key(m, "b", 2), new Map([["a", 1], ["b", 2]]));
  assert.deepEqual(m, new Map([["a", 1]]));
  assert.equal(await s.sum_keys(new Map([["a", 1], ["b", 2]])), "ab");
});

test("nested options stay distinct", async () => {
  assert.equal(await s.depth(await s.nested(null)), "some(none)");
  assert.equal(await s.depth(await s.nested(3)), "some(some(3))");
  assert.equal(await s.depth(null), "none");
  assert.equal(await s.nested(3), 3);
});

test("lists", async () => {
  assert.equal(await s.get_or([1, 2], 1, 9), 2);
  assert.equal(await s.get_or([1, 2], 2, 9), 9);
  assert.equal(await s.get_or([1, 2], -1, 9), 9);
  assert.equal(await s.at([1, 2], 1), 2);
  await assert.rejects(s.at([1, 2], -1), PanicError);
  assert.deepEqual(await s.countdown(3), [3, 2, 1]);
  assert.equal(await s.early([1, -2, -3]), -2);
  assert.equal(await s.early([1]), 0);
});

test("enums and match", async () => {
  assert.equal(await s.area({ tag: "Dot" }), 0.0);
  assert.equal(await s.area({ tag: "Circle", _0: 2.0 }), 12.0);
  assert.equal(await s.area({ tag: "Rect", _0: 2.0, _1: 3.0 }), 6.0);
  assert.equal(await s.classify(0, "", true), "zero/empty/yes");
  assert.equal(await s.classify(5, "x", false), "many/x/no");
});

test("exceptions", async () => {
  assert.equal(await s.caught("ok!"), "ok 3");
  assert.equal(await s.caught(""), "empty");
  assert.equal(await s.caught("nope"), "bad: nope");
  await assert.rejects(s.thrown("nope"), (e: unknown) => {
    assert.ok(e instanceof Thrown);
    assert.deepEqual(e.value, { tag: "Bad", _0: "nope" });
    return true;
  });
  assert.equal(await s.validated("abc"), "abc");
  assert.equal(await s.validated("abcdef"), "validation failed: `short` rejected the value");
});

test("evaluation order", async () => {
  const said: string[] = [];
  const say = (x: string) => {
    said.push(x);
    return x;
  };
  configure({ tools: { log: { say } } });
  assert.equal(await s.order(true), "ab true");
  assert.deepEqual(said, ["a", "b"]);
  said.length = 0;
  assert.equal(await s.order(false), "ac true");
  assert.deepEqual(said, ["a", "c", "d"]);
});

test("unit functions return nothing", async () => {
  assert.equal(await s.noop(1), undefined);
  assert.equal(await s.noop(-1), undefined);
});
