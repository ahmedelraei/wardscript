# Trust labels

Status: implemented in M4 (`ward_check::trust`). Design notes are in
[decision 007](../decisions/007-trust-labels.md).

Every value has a label, `Trusted` or `Untrusted`. Combining values gives the least
trusted of their labels (`Trusted + Untrusted = Untrusted`). The checker proves that
no `Untrusted` value reaches a **sink** unless it went through `validate`, `approve`
or `declassify` first. If one does, that's **W0107**, with the whole path the data
took.

## Labels in signatures

Only signatures, `let` annotations and record and enum fields carry labels. Labels
inside function bodies are inferred.

| Written | Meaning |
|---|---|
| `x: Untrusted<T>` | untrusted, whatever the caller passes |
| `x: Trusted<T>` | a sink: callers must pass trusted data |
| `x: T` | as trusted as what the caller passes (label-polymorphic) |
| `-> Trusted<T>` | a sink for everything the function returns |
| `-> Untrusted<T>`, `-> T` | the result is as trusted as the data it came from |

A type is `Untrusted` if `Untrusted<...>` appears anywhere in it: `List<Untrusted<String>>`
is an untrusted list. Aliases are expanded (`type Input = Untrusted<String>`).
Labels are per value, not per part: a list holding one untrusted element is
untrusted, and so is every element read from it.

A record field declared `Untrusted<T>` is untrusted when read, whatever the record's
own label. A field declared `Trusted<T>` is a sink when the record is built or the
field assigned. An enum variant's field declared `Untrusted<T>` is untrusted when a
pattern binds it.

## Sources

Untrusted data comes from:

- the result of every `ai fn` call;
- the result of every tool call (`mail.send(...)`), and anything read from it;
- parameters, fields and variables declared `Untrusted<T>`;
- the host, which must vouch for anything that reaches a sink (see below).

## Sinks

- every argument of a tool call, until MCP imports (M7) say which parameters are
  sensitive;
- parameters, return types, `let` variables and record fields declared `Trusted<T>`;
- a parameter that reaches a sink inside its function (this is inferred), so calling
  a helper that sends its argument to a tool needs trusted data too. Callers outside
  Wardscript must vouch for such parameters.

`ai fn` arguments are not sinks: passing untrusted text to a model is the point.
An `ai fn` can't declare a `Trusted` return type.

## Flows

**Explicit flows.** Operators, string templates, method calls, list and record
literals, indexing, field access, `Some` and variants all combine their operands'
labels. Calling a Wardscript function combines the labels of the arguments that
reach its result. Assigning to a field or element (`job.command = x`, `xs[i] = x`)
combines `x` into the variable's label; assigning a whole variable (`x = v`) replaces it.

**Implicit flows.** A condition decides which code runs, so:

- An `if` or `match` on untrusted data produces an untrusted value, and pattern
  bindings take the scrutinee's label.
- Inside a branch or loop controlled by untrusted data, every assignment to a
  variable from outside it becomes untrusted (the *pc label*). A variable
  declared inside the branch isn't affected, since it doesn't outlive the branch.
- A `return` under an untrusted condition makes the function's result untrusted.
- `&&` and `||` evaluate their right side under the left side's label.
- Whether a call marked `?` throws depends on its arguments (for `validate`, on the
  value being validated). Inside a `try`, code after such a call runs under that
  label; the `catch` variable, the `catch` block and the `try`'s value do too.

Calling a sink *under* an untrusted condition with trusted arguments is allowed:
`if reply.contains("urgent") { mail.send(ops, "urgent", fixed_text) }` checks. The
checker tracks what data reaches a sink, not whether the sink is reached.
Whether a function returns normally or throws out of it is not tracked either.

## `validate`, `approve`, `declassify`

Each returns its argument, now `Trusted`, and each is recorded in the audit trace (M6).

| Built-in | When to use it |
|---|---|
| `validate(x, rule)?` | a Wardscript function `rule(x) -> Bool` accepts the value; otherwise it throws. The rule sees the unchecked value, so its parameter mustn't reach a sink |
| `approve(x)` | a human approves the value at runtime (`ApprovalDenied` otherwise) |
| `declassify(x, reason)` | the programmer vouches for the value; `reason` is recorded |

## W0107

```text
[W0107] Error: untrusted data reaches the tool call `sh.run`
   ╭─[ cleanup.ward:9:12 ]
 4 │ pub fn cleanup(reply: Untrusted<String>) {
   │                ────────────┬───────────
   │                            ╰───────────── 1. `reply` is declared `Untrusted`
 6 │     if reply.contains("delete") {
   │        ────────────┬───────────
   │                    ╰───────────── 2. the branch taken depends on this
 7 │         command = "rm -rf /"
   │         ───┬───
   │            ╰───── 3. assigned here, where whether this runs depends on it
 9 │     sh.run(command)
   │            ───┬───
   │               ╰───── 4. passed to `sh.run` here
```

The steps are numbered in the order the data flows; the primary label is where the
untrusted value meets the sink's requirement. Steps in other modules (a helper that
forwards a parameter to a tool) are listed as notes with their `path:line:column`,
and in `--format json` under `notes`. Each label remembers one path, even if the
data could arrive along several.

## At runtime

Generated code checks labels again at the host boundary (see
[runtime](runtime.md#trust-at-the-host-boundary)). A parameter that reaches a sink
only accepts a value wrapped in `wardscript.Trusted(...)`. Calls between Wardscript
functions pass the checked values wrapped in the same way.

The runtime checks tool arguments again too ([sink checks](runtime.md#sink-checks)).
It refuses a tool call when an argument is exactly an untrusted value from the run
that no check cleared.
