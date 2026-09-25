//! TypeScript spellings of Wardscript names, types and type descriptors.

use std::collections::BTreeSet;

use ward_ir::{DefId, ModuleId, Program, Ty};

/// Reserved in JavaScript or TypeScript, or names generated code relies on.
const RESERVED: [&str; 48] = [
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "arguments",
    "eval",
];

/// A Wardscript identifier as a TypeScript identifier.
pub fn ident(name: &str) -> String {
    if RESERVED.contains(&name) || name == "undefined" {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// The value holding a record's or enum's runtime description: `Ticket$`.
pub fn descriptor_name(name: &str) -> String {
    format!("{}$", ident(name))
}

pub fn module_alias(name: &str) -> String {
    format!("_{}", name.replace('.', "_"))
}

/// A JavaScript string literal.
pub fn string(s: &str) -> String {
    let mut out = serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned());
    // JSON allows these; older JavaScript engines don't in string literals.
    out = out
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    out
}

/// Refers to items from inside a module (`current`), or from outside all of them.
pub struct Scope<'p> {
    pub program: &'p Program,
    pub current: ModuleId,
    /// Other modules referred to, which the output must import.
    pub imports: BTreeSet<ModuleId>,
}

impl<'p> Scope<'p> {
    pub fn new(program: &'p Program, current: ModuleId) -> Self {
        Scope {
            program,
            current,
            imports: BTreeSet::new(),
        }
    }

    pub fn qualify(&mut self, module: ModuleId, name: String) -> String {
        if module == self.current {
            name
        } else {
            self.imports.insert(module);
            format!("{}.{name}", module_alias(&self.program.module(module).name))
        }
    }

    pub fn func(&mut self, def: DefId) -> String {
        let name = self
            .program
            .func(def)
            .map_or_else(|| "undefined".to_owned(), |f| ident(&f.name));
        self.qualify(def.module, name)
    }

    pub fn adt(&mut self, def: DefId) -> String {
        let name = ident(self.program.adt_name(def));
        self.qualify(def.module, name)
    }

    /// `Counter$new`: creates an object and runs its `init`, which may await.
    pub fn constructor(&mut self, def: DefId) -> String {
        let name = format!("{}$new", ident(self.program.adt_name(def)));
        self.qualify(def.module, name)
    }

    pub fn adt_descriptor(&mut self, def: DefId) -> String {
        let name = descriptor_name(self.program.adt_name(def));
        self.qualify(def.module, name)
    }

    pub fn is_unit_enum(&self, def: DefId) -> bool {
        self.program.enum_(def).is_some_and(|e| e.is_unit_only())
    }

    /// A type annotation. `generics` names the enclosing item's type parameters.
    pub fn annotation(&mut self, ty: &Ty, generics: &[String]) -> String {
        match ty {
            Ty::Int | Ty::Float => "number".into(),
            Ty::String => "string".into(),
            Ty::Bool => "boolean".into(),
            Ty::Unit => "null".into(),
            Ty::Never => "never".into(),
            Ty::List(t) => format!("ReadonlyArray<{}>", self.annotation(t, generics)),
            Ty::Map(k, v) => format!(
                "ReadonlyMap<{}, {}>",
                self.annotation(k, generics),
                self.annotation(v, generics)
            ),
            Ty::Option(t) => format!("_rt.Opt<{}>", self.annotation(t, generics)),
            Ty::Adt(d, args) => {
                let name = self.adt(*d);
                if args.is_empty() {
                    name
                } else {
                    let args: Vec<String> =
                        args.iter().map(|a| self.annotation(a, generics)).collect();
                    format!("{name}<{}>", args.join(", "))
                }
            }
            Ty::Param(i) => generics
                .get(*i)
                .cloned()
                .unwrap_or_else(|| "unknown".into()),
            Ty::Refined(t, _) => self.annotation(t, generics),
            Ty::Var(_) | Ty::Dynamic | Ty::Error => "any".into(),
        }
    }

    /// The runtime descriptor of a type, e.g. `_rt.List(_rt.Adt(Ticket$))`.
    pub fn descriptor(&mut self, ty: &Ty) -> String {
        match ty {
            Ty::Int => "_rt.Int".into(),
            Ty::Float => "_rt.Float".into(),
            Ty::String => "_rt.String".into(),
            Ty::Bool => "_rt.Bool".into(),
            Ty::Unit => "_rt.Unit".into(),
            Ty::List(t) => format!("_rt.List({})", self.descriptor(t)),
            Ty::Map(k, v) => format!("_rt.Map({}, {})", self.descriptor(k), self.descriptor(v)),
            Ty::Option(t) => format!("_rt.Option({})", self.descriptor(t)),
            Ty::Adt(d, args) => {
                let mut parts = vec![self.adt_descriptor(*d)];
                parts.extend(args.iter().map(|a| self.descriptor(a)));
                format!("_rt.Adt({})", parts.join(", "))
            }
            Ty::Param(i) => format!("_rt.Param({i})"),
            Ty::Refined(t, key) => {
                let inner = self.descriptor(t);
                let Some(r) = self.program.refinement(*key) else {
                    return inner;
                };
                let (name, text) = (r.name.clone(), string(&r.text));
                let schema: Vec<String> = r
                    .schema
                    .iter()
                    .map(|(k, v)| format!("{}: {v}", string(k)))
                    .collect();
                let check = self.qualify(key.module, name);
                format!(
                    "_rt.Refined({inner}, {check} as (v: never) => boolean, {text}, {{{}}})",
                    schema.join(", ")
                )
            }
            Ty::Never | Ty::Var(_) | Ty::Dynamic | Ty::Error => "_rt.Any".into(),
        }
    }

    /// `import * as _support_tickets from "./support/tickets.ts"` for each module
    /// referred to, relative to the current one.
    pub fn import_lines(&self) -> Vec<String> {
        let depth = self.program.module(self.current).name.matches('.').count();
        let up = if depth == 0 {
            "./".to_owned()
        } else {
            "../".repeat(depth)
        };
        self.imports
            .iter()
            .map(|&m| {
                let name = &self.program.module(m).name;
                format!(
                    "import * as {} from \"{up}{}.ts\";",
                    module_alias(name),
                    name.replace('.', "/")
                )
            })
            .collect()
    }
}

/// Whether a value of this type may be `null`, so wrapping it in an `Option` needs
/// `_rt.some`.
pub fn may_be_null(ty: &Ty) -> bool {
    match ty {
        Ty::Refined(t, _) => may_be_null(t),
        Ty::Unit | Ty::Option(_) | Ty::Param(_) | Ty::Dynamic | Ty::Var(_) | Ty::Error => true,
        _ => false,
    }
}

/// Whether `==` on this type can be JavaScript's `===`.
pub fn primitive(ty: &Ty, scope: &Scope) -> bool {
    match ty {
        Ty::Int | Ty::Float | Ty::String | Ty::Bool | Ty::Unit => true,
        Ty::Refined(t, _) => primitive(t, scope),
        Ty::Adt(d, _) => scope.is_unit_enum(*d),
        _ => false,
    }
}
