# WEP 005: Exceptions replace `Result`

**Status:** Final. Accepted 2026-09-24. Supersedes the `Result` type (002) and the
auto-`Ok` return (004).

## Decision

```wardscript
pub fn answer(email: Untrusted<String>, to: String) -> String throws String {
    let body = validate(email, no_links)?
    if body.is_empty() {
        throw "empty reply"
    }
    return body
}

let status = try {
    answer(email, to)?
} catch err {
    "failed: {err}"
}
```

- `-> T throws E` declares what a function may throw; any type can be thrown.
- `throw e` raises. `try { } catch err { }` is an expression that handles.
- **`?` stays as a marker** on every call that may throw, and is required there.
- `Result`, `Ok` and `Err` are removed. `validate` throws a `String`.
- `ai fn` can't declare `throws`; a failed model call is a runtime error.

## Why

`Result<String, String>` doesn't say which half is success, and the failure path
has no visible exit in the body. Java-style `throws` keeps the success type as the
return type and names the error separately. Keeping `?` (like Swift's `try`
marker) means every place a function can exit early stays visible to readers and,
later, to the audit trace. The checker still guarantees nothing is silently
ignored: every thrown type is either caught or declared.

## Consequences

- Python codegen (M3) maps this directly onto Python exceptions.
- W0118 now means "`?` on something that can't throw". New codes: W0128 (missing
  `?`), W0129 (unhandled), W0130 (`ai fn throws`), W0131 (useless `try`, warning).
- `ModuleTypes::throws` records the error type of each `?` call for codegen.
