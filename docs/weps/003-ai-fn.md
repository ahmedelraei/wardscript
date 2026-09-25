# WEP 003: `ai fn` replaces `by llm`

**Status:** Final. Accepted 2026-09-24. Supersedes the `by llm "prompt"` form from 001.

## Decision

Model-backed functions are declared with an `ai` modifier, and their body is the
prompt:

```wardscript
ai fn draft_reply(email: Untrusted<String>) -> Reply {
    "Write a short, polite reply to this email:\n{email}"
}
```

- The body is exactly one string literal (W0016); the return type is required (W0017).
- `ai fn` implies the `llm` effect, so `uses {llm}` isn't repeated on every one.
- `ai` becomes a keyword; `by` and `llm` are ordinary identifiers again (`llm`
  remains the effect's name).

## Why

The old form put the one fact that matters most about the function, that a model
answers it and its result is untrusted, on the last line of the header. `ai fn`
shows it where the reader starts, the same way `pub` or `async` do in other
languages. The body keeps the shape of every other function, so the pretty-printer
and readers treat all functions alike.

## Consequences

- Diagnostic codes W0016, W0017 and W0120 keep their meaning; only the wording changed.
- `ai` can no longer be used as a variable name.
