# 008: Effects, budgets and the Rule of Two

**Status:** accepted, 2026-09-24.

## Decision

- **Effects are `llm`, a tool, or one tool function**, named through the declaring
  module's imports. No `uses` means no effects. Effects are transitive: a function
  declares what its callees do too.
- **Budgets are literals**, checked where they can be (the fewest model calls a
  function certainly makes, callee budgets larger than the caller's) and counted by
  the runtime otherwise. `calls` counts model requests, retries included; tool calls
  don't count.
- **The Rule of Two is checked over the call graph**: untrusted input, private reads
  and external changes in one function, counting callees. It's reported where the
  three first meet, and `@allow(rule_of_two, reason = "...")` overrides it.
- **Tool functions are classified by annotations on the import** (`@private(...)`,
  `@readonly(...)`) until M7 reads tool schemas. Unclassified functions count as
  external changes, the conservative default M7 will keep.
- **Using a private read's result isn't "untrusted input".** Private data is the
  system's own, so it counts as the second capability, not the first. The trust
  checker still treats every tool result as untrusted.

## Why

Naming effects by the caller's import keeps them visible in the file that declares
them: a caller can't claim an effect it has no name for. The cost is an import in a
module that only uses a tool through a helper, which is also what makes the
dependency obvious.

Budgets as literals keep them readable, and let the checker compare them. Counting
requests rather than tool calls matches the example budgets (`calls: 3` on an
`ai fn` bounds its retries).

Reporting a Rule of Two violation only where it first appears avoids repeating it in
every caller; an `@allow` there is the one place a human wrote down why it's fine.

## Not yet

- Tool classification and effects from MCP schemas and a lockfile (M7).
- Budgets on tool calls, and interrupting a call that runs past `time`.
- Token estimates from a real tokenizer.
