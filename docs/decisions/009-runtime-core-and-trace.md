# 009: The Rust runtime core and the audit trace

**Status:** accepted, 2026-09-24.

## Decision

- **The Rust core is budget counting and the audit trace** (`ward_runtime`): run ids,
  digests, the event schema, writing JSON Lines, `ward trace show` and the OTLP
  export. `ward_runtime_py` exposes it to Python as `wardscript._core` through PyO3,
  built by maturin into an abi3 wheel (one wheel for Python 3.10+).
- **Everything that calls host code stays in Python**: models, tools, approvers,
  decoding and schemas. The Python API didn't change.
- **A pure-Python core with the same interface** (`_core_py`) is used when the native
  module isn't there. `ward run` needs it, because it embeds the package as source,
  and so does working from the source tree. Tests check both cores produce the same
  digests, records and budget decisions; CI runs the suites against the wheel too.
- **A run is one call from the host.** Every generated function enters `_rt.call`,
  and the outermost one starts and ends the run.
- **Provenance by digests.** Sources (model outputs, tool results, host arguments)
  and checks record digests of a value and its parts; tool calls record their
  arguments' digests. `ward trace show` joins them. Nothing tracks values through
  computation at run time.
- **Async hooks run on their own event loop**, since generated code is synchronous.
- **Providers are thin**: they turn an `AiRequest` into one API call with structured
  output and report usage. Prices are the caller's, since they change.

## Why

The core that CLI tools and the host both need is the trace format: `ward trace`
reads what the runtime writes, so one implementation defines it. Model and tool calls
are host objects, and moving them behind FFI would add complexity for no gain.

Digests give the provenance path the milestone asks for (source → check → sink)
without wrapping every value, which would change what hosts see. They only match
exact values, which is what checks and sinks usually see.

## Not yet

- Async generated code, and streaming.
- Sending traces to a collector directly (export prints OTLP/JSON).
- Recording static labels next to runtime values, and re-checking them at sinks.
