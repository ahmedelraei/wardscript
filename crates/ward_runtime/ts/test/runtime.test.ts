// The runtime on its own: providers against a local HTTP server, the MCP client
// against the inbox example's server, and decoding.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";
import { test } from "node:test";

import { Anthropic, McpServer, ModelError, OpenAI, RateLimited, Thrown, decode, encode, jsonSchema } from "../src/index.ts";
import * as rt from "../src/rt.ts";

async function serve(handler: (body: Record<string, unknown>) => [number, unknown]) {
  const requests: Record<string, unknown>[] = [];
  const server = createServer((req, res) => {
    let data = "";
    req.on("data", (c) => (data += c));
    req.on("end", () => {
      const body = JSON.parse(data);
      requests.push({ ...body, headers: req.headers });
      const [status, reply] = handler(body);
      res.writeHead(status, { "content-type": "application/json" });
      res.end(JSON.stringify(reply));
    });
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  return { url, requests, close: () => server.close() };
}

const REQUEST = { function: "f", prompt: "p", schema: { type: "integer" }, attempt: 0, errors: [] };

test("anthropic: a tool call with the schema, usage and cost", async () => {
  const s = await serve(() => [200, { content: [{ type: "tool_use", input: { value: 7 } }], usage: { input_tokens: 100, output_tokens: 20 } }]);
  try {
    const model = new Anthropic("claude-x", { baseUrl: s.url, apiKey: "k", prices: [3, 15] });
    const answer = await model.complete(REQUEST);
    assert.deepEqual(answer, { text: "7", tokens: 120, cost: (100 * 3 + 20 * 15) / 1e6 });
    const sent = s.requests[0] as { tools: { input_schema: unknown }[]; tool_choice: unknown; headers: Record<string, string> };
    assert.deepEqual(sent.tool_choice, { type: "tool", name: "answer" });
    assert.deepEqual(sent.tools[0]?.input_schema, { type: "object", properties: { value: { type: "integer" } }, required: ["value"], additionalProperties: false });
    assert.equal(sent.headers["x-api-key"], "k");
  } finally {
    s.close();
  }
});

test("openai: a JSON-schema response format", async () => {
  const s = await serve(() => [200, { choices: [{ message: { content: '{"value": [1, 2]}' } }], usage: { prompt_tokens: 10, completion_tokens: 2 } }]);
  try {
    const answer = await new OpenAI("m", { baseUrl: s.url, apiKey: "k" }).complete(REQUEST);
    assert.deepEqual(answer, { text: "[1,2]", tokens: 12, cost: null });
  } finally {
    s.close();
  }
});

test("HTTP errors become model errors", async () => {
  for (const [status, cls] of [[429, RateLimited], [400, ModelError]] as const) {
    const s = await serve(() => [status, { error: "no" }]);
    try {
      await assert.rejects(new Anthropic("m", { baseUrl: s.url }).complete(REQUEST), (e: unknown) => {
        assert.ok(e instanceof cls);
        assert.equal((e as ModelError).status, status);
        return true;
      });
    } finally {
      s.close();
    }
  }
});

test("mcp: lists and calls the example server's tools", async () => {
  const server = join(import.meta.dirname, "..", "..", "..", "..", "examples", "inbox", "gmail_server.py");
  const mail = new McpServer(process.env.WARD_PYTHON ?? "python3", [server]);
  try {
    const tools = await mail.listTools();
    assert.deepEqual(tools.map((t) => t.name), ["list_messages", "read_message", "label_message", "send_email"]);
    assert.equal(await mail.callTool("list_messages", { query: "x", limit: 2 }), "m1\nm2");
    await assert.rejects(mail.callTool("read_message", { id: "nope" }), (e: unknown) => e instanceof Thrown && e.value === "no message nope");
  } finally {
    mail.close();
  }
});

test("decoding and encoding round-trip", () => {
  const Color$ = rt.enum_("Color", () => [["Red", []], ["Green", []]]);
  const Shape$ = rt.enum_("Shape", () => [["Dot", []], ["Circle", [rt.Float]]]);
  const Page$ = rt.record("Page", () => [["items", rt.List(rt.Param(0))], ["next", rt.Option(rt.Int)], ["tags", rt.Map(rt.String, rt.Adt(Color$))]]);
  const t = rt.Adt(Page$, rt.Adt(Shape$));
  const json = { items: ["Dot", { Circle: [2.5] }], next: null, tags: { a: "Red" } };
  const value = decode(t, json) as { items: unknown[]; tags: Map<string, string> };
  assert.deepEqual(value.items[1], { tag: "Circle", _0: 2.5 });
  assert.equal(value.tags.get("a"), "Red");
  assert.deepEqual(encode(value), json);
  const schema = jsonSchema(t) as { $defs: Record<string, unknown> };
  assert.deepEqual(Object.keys(schema.$defs), ["Page_Shape", "Shape", "Color"]);
  assert.throws(() => decode(t, { ...json, next: "x" }), /\$\.next: expected an integer/);
});

test("values print like the other runtimes", () => {
  assert.equal(rt.floatStr(1), "1.0");
  assert.equal(rt.floatStr(0.5), "0.5");
  assert.equal(rt.toStr(rt.some(null)), "None");
  assert.equal(rt.strLen("héllo👋"), 6);
  assert.deepEqual(rt.lines("a\r\nb\n"), ["a", "b"]);
  assert.equal(rt.idiv(-7, 2), -3);
  assert.equal(rt.irem(-7, 2), -1);
});
