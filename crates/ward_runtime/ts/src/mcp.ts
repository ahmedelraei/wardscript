// A minimal MCP client over stdio, so `import mcp "gmail" as mail` can call a real
// MCP server. Servers come from `mcp.json`, as for other MCP clients:
//
//   configure({ tools: loadConfig("mcp.json") })
//
// A result is its `structuredContent` when there is one, else its text; a result with
// `isError` throws `Thrown` with the text, which Wardscript code can catch.

import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { createInterface } from "node:readline";

import { Thrown, ToolError } from "./errors.ts";

export const PROTOCOL_VERSION = "2025-06-18";

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

export class Server {
  readonly command: string;
  readonly args: string[];
  readonly env: Record<string, string>;
  readonly cwd: string | undefined;
  readonly name: string;
  private proc: ChildProcessWithoutNullStreams | null = null;
  private ready: Promise<void> | null = null;
  private nextId = 0;
  private readonly pending = new Map<number, Pending>();

  constructor(command: string, args: string[] = [], options: { env?: Record<string, string>; cwd?: string; name?: string } = {}) {
    this.command = command;
    this.args = args;
    this.env = options.env ?? {};
    this.cwd = options.cwd;
    this.name = options.name ?? command;
  }

  private start(): Promise<void> {
    if (this.ready) return this.ready;
    const proc = spawn(this.command, this.args, {
      cwd: this.cwd,
      env: { ...process.env, ...this.env },
      stdio: ["pipe", "pipe", "inherit"],
    }) as unknown as ChildProcessWithoutNullStreams;
    this.proc = proc;
    proc.on("error", (e) => this.failAll(new ToolError(`MCP server \`${this.name}\` didn't start: ${e.message}`)));
    proc.on("exit", () => this.failAll(new ToolError(`MCP server \`${this.name}\` exited`)));
    createInterface({ input: proc.stdout }).on("line", (line) => this.onLine(line));
    this.ready = this.request("initialize", {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: {},
      clientInfo: { name: "wardscript", version: "0.1.0-beta.1" },
    }).then(() => this.send({ jsonrpc: "2.0", method: "notifications/initialized" }));
    return this.ready;
  }

  private failAll(e: Error): void {
    for (const p of this.pending.values()) p.reject(e);
    this.pending.clear();
  }

  private send(message: unknown): void {
    this.proc?.stdin.write(`${JSON.stringify(message)}\n`);
  }

  private onLine(line: string): void {
    let message: { id?: number; method?: string; result?: unknown; error?: { message?: string } };
    try {
      message = JSON.parse(line);
    } catch {
      return; // Not protocol output.
    }
    if (message.method !== undefined) {
      if (message.id !== undefined) {
        this.send(
          message.method === "ping"
            ? { jsonrpc: "2.0", id: message.id, result: {} }
            : { jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "not supported" } },
        );
      }
      return;
    }
    const p = message.id !== undefined ? this.pending.get(message.id) : undefined;
    if (!p || message.id === undefined) return;
    this.pending.delete(message.id);
    if (message.error) p.reject(new ToolError(`MCP server \`${this.name}\`: ${message.error.message ?? "error"}`));
    else p.resolve(message.result);
  }

  private request(method: string, params: unknown): Promise<unknown> {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.send({ jsonrpc: "2.0", id, method, params });
    });
  }

  async listTools(): Promise<Record<string, unknown>[]> {
    await this.start();
    const tools: Record<string, unknown>[] = [];
    let cursor: string | undefined;
    do {
      const result = (await this.request("tools/list", cursor ? { cursor } : {})) as { tools?: Record<string, unknown>[]; nextCursor?: string };
      tools.push(...(result.tools ?? []));
      cursor = result.nextCursor;
    } while (cursor);
    return tools;
  }

  async callTool(name: string, args: Record<string, unknown>): Promise<unknown> {
    await this.start();
    const result = (await this.request("tools/call", { name, arguments: args })) as {
      content?: { type: string; text?: string }[];
      structuredContent?: unknown;
      isError?: boolean;
    };
    const text = (result.content ?? []).filter((c) => c.type === "text").map((c) => c.text ?? "").join("\n");
    if (result.isError) throw new Thrown(text || `\`${name}\` failed`);
    return result.structuredContent ?? text;
  }

  close(): void {
    this.proc?.stdin.end();
    this.proc?.kill();
    this.proc = null;
    this.ready = null;
  }
}

/** The stdio servers of an `mcp.json`, by name. Relative paths run from its directory. */
export function loadConfig(path = "mcp.json"): Record<string, Server> {
  const config = JSON.parse(readFileSync(path, "utf8")) as { mcpServers?: Record<string, { command?: string; args?: string[]; env?: Record<string, string> }> };
  const cwd = dirname(resolve(path));
  const out: Record<string, Server> = {};
  for (const [name, spec] of Object.entries(config.mcpServers ?? {})) {
    if (!spec.command) continue;
    out[name] = new Server(spec.command, spec.args ?? [], { env: spec.env, cwd, name });
  }
  return out;
}
