# Names, modules and imports

Status: implemented in M2 (`ward_resolve`).

## Modules

One file is one module. `import a.b` loads `a/b.wardscript`, relative to the
directory of the entry file, and binds it as `b` (or as the name given with
`as`). Modules may import each other in cycles.

Only items marked `pub` can be used from another module (W0104). Imports themselves
are never visible outside the module that declares them.

```wardscript
import support.tickets as t

fn escalate(ticket: t.Ticket) -> t.Ticket {
    t.Ticket { title: ticket.title, priority: t.Priority.High }
}
```

`import mcp "server" as x` declares a tool namespace. Until M7 reads tool schemas,
calls like `x.send(...)` accept any arguments and return a *dynamic* value that is
accepted anywhere; fields and methods on it are dynamic too.

## Scopes

- Module scope holds every item (`fn`, `type`, `enum`) and import. Types and values
  share one namespace, so a function and a record can't have the same name (W0103).
- The prelude sits behind module scope and can be shadowed. It holds the types
  `Int Float String Bool List Map Option Result Untrusted Trusted` and the values
  `Some None Ok Err validate approve declassify`.
- Function parameters, `let` bindings, `for` variables and pattern bindings are local.
  Every block opens a scope. `let` may shadow an earlier variable.
- Enum variants are reached through their enum: `Priority.High`. The prelude's
  `Some`, `None`, `Ok` and `Err` are the exception.
- In a pattern, a lone name is a new binding, except `None`.

## Record literals

`Name { field: value, other }` builds a record. `other` alone is shorthand for
`other: other`. A record from another module is written `module.Name { ... }`.
