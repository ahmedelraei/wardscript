# 016: The language server

**Status:** accepted, 2026-09-24.

## Decision

- **`ward lsp`**, in its own crate (`ward_lsp`), speaks the Language Server
  Protocol over stdio: diagnostics on open, change and save; the type of the
  expression under the cursor (a function's signature on its name) on hover; and
  go to definition for variables, functions, variants, types and tool imports.
- **Hand-written, synchronous JSON-RPC** instead of `tower-lsp`: the server checks
  one file at a time and answers in order, so an async runtime would add
  dependencies without adding anything yet.
- **Each open file is the entry of its own program.** Imports are read from open
  editors first, then from disk, so a change to an imported module shows up in
  the importing file when it is next checked. Only the entry file's diagnostics
  are published for it.
- **`editors/vscode`** has a TextMate grammar (including the contextual keywords
  `model`, `check`, `where`, `test`, `assert`) and a small client that starts
  `ward lsp`. A test keeps the grammar in sync with the keyword list in the spec.

## Not done

- Incremental checking (`salsa`); every change re-checks the file and its imports.
- Completion, rename, find references, formatting (the printer drops comments).
- Publishing to the VS Code Marketplace.
