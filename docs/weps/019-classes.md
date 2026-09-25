# WEP 019: Classes

**Status:** Final. Accepted 2026-09-25.

## Decision

- **Classes are a separate kind of type from records.** Records stay immutable
  values; a class describes objects, shared by reference and changed in place. Data
  that crosses the model or the host boundary stays in records, which have a JSON
  schema; objects don't.
- **Full classes**: fields, `init`, methods (including `ai fn` methods), single
  inheritance with `open` classes, `open`/`override` methods, `super`, and members
  private unless `pub`. Private means visible in the class's and its subclasses'
  methods, which is all the methods of one hierarchy can be expected to share.
- **Nothing is overridable by default**, and an override says `override`: a
  subclass can only change what the base class chose to let it change, and nobody
  overrides by accident.
- **A field's label is its declaration's.** A variable's label can't describe an
  object that other variables also hold. So a field declared `Untrusted<T>` may hold
  anything and always reads as untrusted, and every other field only holds trusted
  data: storing into it is a sink for the value, the pc label and the reference.
  This is the Jif-style static field label, restricted to the two labels
  Wardscript has.
- **Heap writes under untrusted control are rejected**, not only direct stores: a
  function's summary records whether it writes a trusted-only field, and calling it
  under an untrusted pc is W0107. Otherwise `if secret { flag.set() }` would move one
  bit per call into a field that reads as trusted. `init` writing its own new object
  is exempt, since nothing else can observe it.
- **Virtual calls join over every override** in the program, for trust summaries,
  effects and minimum model calls. The whole program is checked together, so the
  set is known; this is more precise than requiring overrides to fit the base
  method's contract, and needs no annotations on the base.
- **Methods are functions.** The parser places each method right after its class as
  an ordinary `Item::Fn` with an implicit `self` parameter, so resolution, type
  checking, trust summaries, effects, budgets and the Rule of Two handle them without
  special cases; only calls (dispatch) and field stores are new.
- **`init` must set every own field, at the top level of its body**, and a subclass's
  `init` starts with `super.init(...)` when a base has one. A syntactic rule is easy
  to explain and to check, and keeps the TypeScript output valid (fields are
  declared `name!: T`).
- **Constructors don't await in either backend**: synchronous Python uses
  `__init__`; async Python uses `await Class._new(...)` and TypeScript
  `await Class$new(...)`, which run the `init` method. Each class's `init` has its
  own TypeScript name (`_init$Class`) so a subclass can take other parameters.

## Interfaces and abstract classes (added the same day)

- **Interfaces are nominal**: a class says which it implements, after its base class
  (`class Square: Shape, Named`), and implements each method with `override`, so a
  signature change in the interface points at every implementation.
- **Abstract classes are implicitly open**, and so are their abstract methods: there
  is no other use for them.
- **Calls through an interface join over every implementation**, like virtual calls.
- **A method's trusted parameters are shared by its whole override family.** A caller
  vouches for arguments according to the method it names, and the callee checks them
  according to its own summary; if an override sends a parameter to a sink that the
  base method doesn't, the two must agree, or the override would reject the
  unvouched value at runtime. The static check already joined over the family.
- In Python, interfaces are `typing.Protocol`s rather than bases, which avoids method
  resolution order conflicts when a class and its base both name one; in TypeScript
  they are interfaces, which don't exist at runtime either.

## Why not methods on records

Methods on immutable records, plus interfaces, would have been smaller and wouldn't
need field labels. But the programs this language is for keep state across calls
(an agent's memory, a conversation, a budget), and threading a record through every
function by hand is what people reach for classes to avoid.

## Not yet

- Generic classes and interfaces.
- Subtyping inside other types (`List<B>` as `List<A>`), downcasts and `is` tests.
- Definite-assignment analysis beyond "assigned at the top level of `init`".
- Calling a method that writes the object from `init` counts as a heap write, so
  constructing such an object under an untrusted condition is rejected.
