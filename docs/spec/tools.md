# Tools and MCP imports

Status: implemented in M7 (`ward_resolve::tools`, `ward_check::tools`, `ward lock`,
`wardscript.mcp`). Design notes are in
[WEP 012](../weps/012-mcp-imports.md).

```ward
@private(list_messages, read_message)
@not_sink(label_message.label, reason = "labels are only visible to the mailbox owner")
import mcp "gmail" as mail
```

`import mcp "gmail" as mail` names a tool server. Its functions are called like
`mail.send_email(to, subject, body)?`.

## Schemas: `mcp.json` and `ward.lock`

Servers are listed the way other MCP clients list them, in `mcp.json`:

```json
{"mcpServers": {"gmail": {"command": "python3", "args": ["gmail_server.py"], "env": {}}}}
```

`ward lock [mcp.json]` starts each stdio server, asks for its tools
(`tools/list`) and writes `ward.lock` next to the config: each tool's `name`,
`title`, `description`, `inputSchema`, `outputSchema` and `annotations`, sorted, so
builds are reproducible and schema changes show up in review. `ward lock --check`
writes nothing and exits with 1 when the lock is out of date (for CI). Servers
reached over HTTP (`url`) are skipped for now.

`ward check` reads `ward.lock` from the entry file's directory or the nearest
parent that has one. A lock that can't be read is an error at every tool import
(W0300). An import whose source isn't in the lock is a warning (W0301) and stays
*dynamic*, as when there is no lock at all: its calls take any arguments and
return a value accepted anywhere, and every argument is a sink.

## Typed calls

With a schema, `mail.f` must be one of the server's tools (W0105, with a
suggestion). A tool name that isn't a Wardscript name is changed: characters other
than letters, digits and `_` become `_` (`send-email` is `send_email`), a leading
digit gets a `_` before it, and a keyword gets one after it.

| JSON Schema | Wardscript |
|---|---|
| `string` (and string `enum`s) | `String` |
| `integer` | `Int` |
| `number` | `Float` |
| `boolean` | `Bool` |
| `array` with `items` | `List<T>` |
| `["T", "null"]`, or `anyOf`/`oneOf` of one type and `null` | `Option<T>` |
| `object`, and anything else | dynamic |

Parameters are positional: the required ones in the order the schema lists them,
then the optional ones. An optional parameter has type `Option<T>`, and optional
parameters at the end may be left out (W0115 otherwise).

The result is the `outputSchema`'s type, or `String` (the text content) when the
tool has none. Every typed tool call can throw a `String`, the message of a
failed call (MCP's `isError`), so it needs `?` (W0128) or a `try`.

## Trust

A tool's result is always `Untrusted`, whatever the schema says.

Which parameters are **sinks**:

- every parameter of a tool that may change something or reach the outside world;
- none of a tool whose annotations say it is read-only and closed-world
  (`readOnlyHint: true` and `openWorldHint: false`);
- `@sink(f.p, ...)` makes parameters sinks; `@not_sink(f.p, ..., reason = "...")`
  frees them, with a reason for reviewers.

Without a schema, every argument is a sink. `@sink` and `@not_sink` need a schema,
and name parameters that exist (W0221).

## Rule of Two

A tool function is classified for the [Rule of Two](effects.md#the-rule-of-two) by
its annotations on the import, else by its schema: `readOnlyHint: true` makes it
read-only; anything else changes external state. `@private(f)` marks a tool that
reads private data, and `@readonly(f)` one that doesn't change anything.

## At run time

The generated code passes the schema to the runtime: the parameter names, which
are sinks, and the result's type. A tool configured as an object with a
`call_tool(name, arguments)` method, like `wardscript.mcp.Server`, gets named
arguments (optional ones that are `None` are left out); plain functions still get
positional ones. The result is decoded as the schema's type, or the call raises
`ToolError`. Only sink parameters get the runtime [sink check](runtime.md#sink-checks).

```python
from wardscript import mcp, runtime

runtime.configure(tools=mcp.load_config("mcp.json"))
```

`ward run` does this itself with the nearest `mcp.json`.
