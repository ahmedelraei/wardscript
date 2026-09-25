# Classes

Status: implemented (WEP [019](../weps/019-classes.md)).

A **class** describes objects: values with fields that methods read and change,
shared by reference. Records stay what they were, immutable values that are copied
when a field is assigned; use a class when several parts of a program need to see
the same changing state, like an agent's memory or a counter.

```ward
pub open class Agent {
    pub name: String
    notes: Untrusted<List<String>>
    calls: Int

    pub init(name: String) {
        self.name = name
        self.notes = []
        self.calls = 0
    }

    pub open fn handle(text: Untrusted<String>) -> String {
        self.calls = self.calls + 1
        self.notes = self.notes.push(text)
        "{self.name} has {self.notes.len()} notes"
    }

    pub ai fn summarize() -> String {
        "Summarize these notes:\n{self.notes}"
    }
}

pub class Shouter: Agent {
    pub override fn handle(text: Untrusted<String>) -> String {
        super.handle(text).upper()
    }
}

pub fn main() -> String {
    let a: Agent = Shouter("bot")
    a.handle("hi")
}
```

## Declaring a class

```ebnf
class       = ["pub"] (["open" | "abstract"] "class" | "interface") IDENT
              [":" type ("," type)*] "{" member* "}" ;
member      = annotation* ["pub"] ( field | init | method ) ;
field       = IDENT ":" rtype [","|";"] ;
init        = "init" "(" [param ("," param)* [","]] ")" ["throws" type] clause* block ;
method      = ["open"] ["override"] ["ai"] "fn" IDENT signature (block | "{" STRING "}")
            | "abstract" "fn" IDENT signature ;   (* in an interface, every `fn` is abstract *)
```

`class`, `interface`, `open`, `abstract`, `override` and `init` are only keywords there; elsewhere they are
ordinary names. Members go one per line.

- **Fields** have a type, and a label like record fields ([trust](#trust)).
  Refinements aren't allowed on them (W0133): they describe decoded data.
- **Methods** are functions with an implicit first parameter `self`, the object.
  They take everything a function takes: `throws`, `uses`, `budget`, annotations,
  and `ai fn` methods, whose prompt can read `self`'s fields.
- **`init`** sets the object up. It must assign every field the class declares,
  with `self.field = ...` statements directly in its body, not inside an `if` or a
  loop (W0143). A class with fields needs an `init`. `init` returns nothing (W0145).
- Classes and interfaces can't be generic yet (W0145).

## Creating and using objects

`Agent("bot")` creates an object: it runs the class's `init`, or the nearest base
class's if the class declares none, with the arguments given, and returns the
object. A class without any `init` takes no arguments. Calling `init` on an object is
an error (W0145).

`obj.field` reads a field and `obj.field = value` changes it in place: every
variable holding the object sees the change. `obj.method(args)` calls a method.
Methods and fields are looked up in the object's class, then its bases.

`==` on objects compares identity. Objects can't be matched with patterns and have no
JSON schema, so an `ai fn` can't return one (W0120); use a record for data that
crosses the model or the host boundary.

## Visibility

Members are private unless marked `pub`: a private field, method or `init` can only
be used in the methods of its class and of its subclasses (W0142). Across modules,
the class itself must be `pub`, as any item.

## Inheritance

`class B: A` extends `A`, which must be declared `open class` or `abstract class` (W0140). A class has
one base. An object of `B` is accepted wherever an `A` is expected (a parameter, a
`let` with a type, a list element checked against `List<A>`, a return value).
Otherwise the types must match exactly: `List<B>` isn't a `List<A>`.

- A method can be **overridden** only if it is `open`, abstract, or itself an `override`, and
  the overriding method must say `override` (W0141). It takes the same parameters,
  returns and throws the same types, and has the same visibility.
- Calls are **virtual**: `a.handle(x)` runs the override for the class `a`'s object
  was created from.
- **`super.name(...)`** calls the base class's method itself, not an override. It
  is only available in methods of a class that has a base (W0144).
- A subclass's `init` must begin with **`super.init(...)`** when a base class has
  an `init`, and `super.init` may appear nowhere else (W0144). A subclass without
  its own fields may leave out `init` and inherit the base's.

## Interfaces and abstract classes

```ward
pub interface Named {
    fn name() -> String
}

pub abstract class Shape: Named {
    pub abstract fn area() -> Float

    pub override fn name() -> String {
        "shape"
    }
}

pub class Square: Shape, Named {
    side: Float

    pub init(side: Float) {
        self.side = side
    }

    pub override fn area() -> Float {
        self.side * self.side
    }
}
```

- An **interface** declares methods without bodies: `fn name(params) -> T`,
  optionally `throws E`, and nothing else (no fields, `init`, clauses or `ai fn`).
  Its methods are public. It can extend other interfaces:
  `interface Greeter: Named, Polite`.
- An **abstract class** is a class that can't be created (W0147) and that may declare
  **abstract methods**, `abstract fn` with a signature and no body. It can be
  extended without being declared `open`, and its abstract methods can be overridden
  without being declared `open`.
- After `:`, a class lists **its base class first, then the interfaces** it
  implements (W0140). An object is accepted where any of its supertypes is expected.
- A class implements an interface's or abstract class's method with a `pub override
  fn` of the same signature (W0141). A class that can be created must implement every
  abstract method it inherits, from its base classes and all its interfaces (W0146).
  One method can implement a base class's and an interface's method of the same name
  at once.
- An interface or abstract method has nothing to run, so `super.name()` can't call
  one (W0144). A call through an interface or abstract class runs the implementation
  of the object's class, and, for trust and effects, counts as a call to every
  implementation in the program.

## Trust

Objects are shared, so a variable's label can't describe a field: another variable
may hold the same object. Instead, **a field's label is its declaration's**:

- A field declared `Untrusted<T>` may hold untrusted data, and reading it is always
  untrusted.
- Any other field only ever holds trusted data. Storing into it is a sink: the value,
  the condition under which the store runs (the pc label) and the reference to the
  object must all be trusted, else W0107 ("untrusted data reaches the field
  `Draft.body`, which isn't declared `Untrusted`"). Reading it gives the label of
  the reference it's read through.

As with any sink, a method that stores its parameter in such a field makes that
parameter a sink, so its callers must pass trusted data. A constructor's arguments
usually are: `init` stores them.

Whether a method that writes such a field runs must not depend on untrusted data
either: calling it inside a branch on untrusted data is W0107, since the field's
value would reveal the branch. `init` writing the fields of the object it's creating
doesn't count, because nothing else can see that object yet.

A virtual call may run any override in the program, so it takes the requirements
and the result labels of all of them: if one override sends a parameter to a tool,
calls through the base class must pass trusted data too.

## Effects

A method's effects are declared and checked like a function's. A virtual call uses
the effects of every override, so the caller declares them all; the base method
itself need not.

## Generated code

**Python.** A class becomes a Python class with the same name and base; fields are
annotated in the class body. An interface becomes a `typing.Protocol`, which classes
match by their methods instead of listing it as a base, and an abstract method
raises `NotImplementedError`. `init` becomes `__init__`, so hosts create objects with
`Agent(...)`, and methods keep their names. Arguments that reach a sink need
`wardscript.Trusted(...)`, as for functions. With `--async`, constructors can't
await, so `init` becomes `async def _init`, and objects are created with
`await Agent._new(...)`.

**TypeScript.** A class becomes an exported (when `pub`) class, `implements` its
interfaces, which become TypeScript interfaces, and an abstract class and methods
stay `abstract`; fields are declared `name!: T`. Constructors can't be async, so an object is created with
`await Agent$new(...)`, which calls the class's `init`, generated as the method
`_init$Agent`.

Each method call is a run in the audit trace, named `Class.method`.
