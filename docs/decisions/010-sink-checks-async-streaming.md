# 010: Runtime sink checks, collectors, async code and streaming

**Status:** accepted, 2026-09-25.

## Decision

- **Sink checks use the trace's digests.** Before a tool call, the runtime compares each
  argument and its parts with the digests of untrusted sources and cleared values
  already in the run's records. An exact match with an uncleared source stops the
  call with `TrustError`. The check is on by default, and `check_sinks=False` turns
  it off.
- **Only exact matches.** Tracking labels through every operation at run time would
  mean wrapping every value in the generated code, which is slow and makes it
  harder to read. The checker covers derived values. The runtime check is a backstop
  for the direct case: a checker bug, or generated code that was edited.
- **Short scalars are ignored** (strings under 8 characters, numbers, booleans): a
  literal `"yes"` in the program and a model answering `"yes"` would otherwise
  collide.
- **Collectors get OTLP/HTTP with JSON**, from `ward trace export --endpoint` or from
  the runtime at the end of each run (`otlp_endpoint`, or the standard
  `OTEL_EXPORTER_OTLP_ENDPOINT`). A failed send only warns. The OTLP export is also
  in the Python core now, and a parity test compares it with the Rust one.
- **Async code is a build option** (`ward build --async`), not the default. It
  generates the same functions as `async def`, and every call that may wait is
  awaited. Hosts that aren't async keep the simpler synchronous build.
- **Streaming is used when something needs it**: an `on_stream` observer, or a
  `tokens` budget that can end the answer early. Otherwise the runtime calls
  `complete`, which is simpler for providers and mocks.

## Why

The trace already records what a sink check needs, so the check reads the trace
instead of adding a second mechanism. Streaming makes `tokens` budgets
enforceable during an answer, not only after one arrives. That matters most for
long answers, which are where a budget saves the most.

## Not done

- Label tracking at run time for derived values.
- gRPC or protobuf OTLP; batching several runs into one request.
- Streaming partial *decoded* values: the observer gets text, and only the finished
  answer is decoded.
