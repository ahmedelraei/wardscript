# 012: MCP imports and `ward.lock`

**Status:** accepted, 2026-09-24.

## Decision

- **Schemas come from a lock, not from the network at check time.** `ward lock`
  asks each server in `mcp.json` (the config format other MCP clients already use)
  for `tools/list` and writes `ward.lock`. `ward check` only reads the lock, so a
  check is offline, fast and reproducible, and a server changing its tools shows
  up as a diff to review. `ward lock --check` guards CI.
- **The lock stores what the server said**, trimmed to the fields that matter
  (`name`, `title`, `description`, `inputSchema`, `outputSchema`, `annotations`),
  with object keys in the server's order: parameters are positional in
  Wardscript, in schema order (required first).
- **No schema stays dynamic**, as before M7, and every argument stays a sink. A
  lock that exists but lacks a server is only a warning (W0301): the fallback is
  safe, just untyped.
- **Results are always untrusted.** The plan allowed a config for trusted
  results; that would be a fourth way to make data trusted besides `validate`,
  `approve` and `declassify`, so it's left out.
- **Sinks are conservative by default.** Every parameter of a tool is a sink unless
  the tool says it is read-only *and* closed-world. A read-only tool that reaches
  the outside world (a web fetch) can leak data through its arguments, so it stays
  a sink. `@sink(f.p)` and `@not_sink(f.p, reason = "...")` adjust single
  parameters; freeing one needs a reason, like `@allow`.
- **Typed tool calls throw `String`.** MCP reports a failed call with `isError`
  and a message; that becomes a Wardscript throw, so programs handle it with `?`
  or `try` instead of crashing with a host exception.
- **The runtime gets the schema too**: parameter names (for MCP's named
  arguments), sink flags (so the runtime sink check skips `@not_sink`
  parameters) and the result's type (checked with the same decoder as model
  answers). `wardscript.mcp` is a small stdio client with no dependencies, and
  `ward run` connects to the nearest `mcp.json` itself.

## Why

Tool imports were the biggest untyped hole: any argument, any result, every
parameter a sink. Schemas make tool calls as checked as the rest of the program,
and per-parameter sinks cut false positives (a read-only search can take an
untrusted query) without weakening the default.

## Not done

- Named record types for object results (they are dynamic for now), and nested
  object parameters.
- Servers over HTTP (`url` in `mcp.json`); only stdio servers are locked and
  called.
- Resources and prompts from MCP servers.
