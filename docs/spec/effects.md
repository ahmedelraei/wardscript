# Effects, budgets and the Rule of Two

Status: implemented in M5 (`ward_check::effects`; budgets enforced by the runtime).
Design notes are in [decision 008](../decisions/008-effects-budgets-rule-of-two.md).

## Effects

A function's `uses {...}` clause lists what it does besides computing a value. A
function without one does nothing else.

| Effect | Covers |
|---|---|
| `llm` | calling an `ai fn` |
| `mail` | every function of the tool imported as `mail` |
| `mail.send` | calls to `mail.send` |

A function's effects are what its own body does plus the effects of every function it
calls (and of the rule passed to `validate`), transitively. It must declare all of
them (W0200). The diagnostic points at the call that needs the effect and suggests the
full clause. An `ai fn` implies `llm` and doesn't declare it.

Effects are named through the function's own imports. When a helper in another
module uses a tool, the caller has to import that tool too, to be able to declare
the effect.

A declared effect nothing uses is a warning (W0201), and so is `uses {llm}` on an
`ai fn`. A name that isn't `llm`, an imported tool or one of its functions is an
error (W0202).

```ward
import mcp "gmail" as mail

pub fn answer(email: Untrusted<String>, to: String) -> String throws String
    uses {llm, mail.send}
{
    ...
}
```

## Budgets

```ward
pub fn handle(email: Untrusted<String>) -> String
    uses {llm, mail}
    budget {tokens: 8000, calls: 5, cost: 0.10, time: 60}
```

| Budget | Limits | Value |
|---|---|---|
| `tokens` | model tokens, prompt and answer | whole number |
| `calls` | model requests, retries included | whole number |
| `cost` | dollars spent on the model | number |
| `time` | seconds of wall-clock time | number |

Values must be non-negative number literals, each set once (W0210). A budget covers
everything the function does, callees included; nested budgets are all charged.

**At compile time**, the checker counts the fewest model calls any run of the
function makes: loop bodies, the branch with fewer calls, the left side only of `&&`
and `||`, nothing in a `try`. If that is more than `calls`, the budget is always
exceeded (W0211). A call to a function whose budget for some resource is larger than
the caller's is warned about (W0212), since the callee can never use all of it.

**At run time**, the runtime counts, and stops the run with `BudgetExceeded` as soon
as a function goes over:

- `calls` before each model request, so a request over the limit is never sent;
- `tokens` and `cost` after each answer, from what the model reports
  ([`Completion`](runtime.md#the-runtime)); without a report, tokens are estimated
  from the text's length. An unknown cost under a `cost` budget stops the run with
  `BudgetUnenforceable`, before the request when the model has no prices
  ([unknown cost](runtime.md#unknown-cost));
- `time` before and after each model and tool call. A call in progress isn't
  interrupted.

## The Rule of Two

A function may not have all three of these capabilities, counting its callees:

| Capability | Comes from |
|---|---|
| processes untrusted input | a parameter declared `Untrusted`, or using the result of an `ai fn` or of a tool function that isn't `private` |
| reads private data | a call to a tool function marked `private` |
| changes external state or communicates | a call to a tool function not marked `private` or `readonly` |

Until tool schemas arrive (M7), tool functions are classified with annotations on the
import. Anything unlisted is assumed to change external state.

```ward
@readonly(get_issue, search)
@private(read_file)
import mcp "github" as gh
```

A violation is W0220, reported on the function where the three first come together,
with where each one comes from. Its callers aren't reported again. If a human
reviews what the function does, it can be allowed, with a reason:

```ward
@allow(rule_of_two, reason = "a human approves the full diff before anything is pushed")
pub fn fix_issue(repo: String, number: Int) -> String throws String
```

A missing or empty reason, or an unknown annotation or argument, is W0221. An
`@allow(rule_of_two)` on a function that doesn't break the rule is a warning
(W0222).

## Annotations

`@name` or `@name(arg, key = "string")` before a function or an import (see
[syntax](syntax.md)). Only these are recognized:

| Annotation | On |
|---|---|
| `@allow(rule_of_two, reason = "...")` | functions |
| `@private(f, ...)`, `@readonly(f, ...)` | tool imports |
