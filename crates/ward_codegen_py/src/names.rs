//! Python spellings of Wardscript names.

use ward_ir::{DefId, ModuleId, Program, Ty};

const KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Builtins that generated code calls by name, so user names must not shadow them.
const BUILTINS: [&str; 5] = ["float", "isinstance", "len", "list", "str"];

/// A Wardscript identifier as a Python identifier: keywords and the builtins generated
/// code relies on get a trailing `_`, as Python programmers do by hand (`from_`).
pub fn ident(name: &str) -> String {
    if KEYWORDS.contains(&name) || BUILTINS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// An attribute name: only keywords need escaping.
pub fn attr(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// The name a generated module imports another one as: `support.tickets` becomes
/// `_support_tickets`.
pub fn module_alias(name: &str) -> String {
    format!("_{}", name.replace('.', "_"))
}

/// The class of a variant of an enum with fields: `Verdict_Fail`.
pub fn variant_class(enum_name: &str, variant: &str) -> String {
    format!("{}_{variant}", ident(enum_name))
}

/// Refers to items from inside a module (`current`), or from outside all of them.
pub struct Scope<'p> {
    pub program: &'p Program,
    pub current: Option<ModuleId>,
    /// Other modules referred to, which the output must import.
    pub imports: std::collections::BTreeSet<ModuleId>,
    pub uses_typing: bool,
}

impl<'p> Scope<'p> {
    pub fn new(program: &'p Program, current: Option<ModuleId>) -> Self {
        Scope {
            program,
            current,
            imports: Default::default(),
            uses_typing: false,
        }
    }

    fn qualify(&mut self, module: ModuleId, name: String) -> String {
        if Some(module) == self.current {
            name
        } else {
            self.imports.insert(module);
            format!("{}.{name}", module_alias(&self.program.module(module).name))
        }
    }

    pub fn item(&mut self, def: DefId, name: &str) -> String {
        self.qualify(def.module, ident(name))
    }

    pub fn adt(&mut self, def: DefId) -> String {
        let name = self.program.adt_name(def).to_owned();
        self.item(def, &name)
    }

    pub fn func(&mut self, def: DefId) -> String {
        let name = self
            .program
            .func(def)
            .map_or_else(|| "?".to_owned(), |f| f.name.clone());
        self.item(def, &name)
    }

    /// `Priority.Low` or `Verdict.Fail`: both work as values and in `isinstance`.
    pub fn variant(&mut self, enum_: DefId, index: usize) -> String {
        let variant = self
            .program
            .enum_(enum_)
            .and_then(|e| e.variants.get(index))
            .map_or_else(|| "?".to_owned(), |v| attr(&v.name));
        format!("{}.{variant}", self.adt(enum_))
    }

    /// A type annotation. `generics` names the enclosing item's type parameters.
    pub fn annotation(&mut self, ty: &Ty, generics: &[String]) -> String {
        match ty {
            Ty::Refined(t, _) => self.annotation(t, generics),
            Ty::Int => "int".into(),
            Ty::Float => "float".into(),
            Ty::String => "str".into(),
            Ty::Bool => "bool".into(),
            Ty::Unit => "None".into(),
            Ty::Never => {
                self.uses_typing = true;
                "_typing.NoReturn".into()
            }
            Ty::List(t) => format!("list[{}]", self.annotation(t, generics)),
            Ty::Map(k, v) => format!(
                "dict[{}, {}]",
                self.annotation(k, generics),
                self.annotation(v, generics)
            ),
            Ty::Option(t) => format!("{} | None", self.annotation(t, generics)),
            Ty::Adt(d, args) => {
                let name = self.adt(*d);
                if args.is_empty() {
                    name
                } else {
                    let args: Vec<String> =
                        args.iter().map(|a| self.annotation(a, generics)).collect();
                    format!("{name}[{}]", args.join(", "))
                }
            }
            Ty::Param(i) => type_var(generics.get(*i).map_or("T", String::as_str)),
            Ty::Var(_) | Ty::Dynamic | Ty::Error => {
                self.uses_typing = true;
                "_typing.Any".into()
            }
        }
    }

    /// The runtime descriptor of a type, e.g. `_rt.List(_rt.Adt(Ticket))`.
    pub fn descriptor(&mut self, ty: &Ty) -> String {
        match ty {
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
                    "_rt.Refined({inner}, {check}, {text}, {{{}}})",
                    schema.join(", ")
                )
            }
            Ty::Int => "_rt.Int".into(),
            Ty::Float => "_rt.Float".into(),
            Ty::String => "_rt.String".into(),
            Ty::Bool => "_rt.Bool".into(),
            Ty::Unit => "_rt.Unit".into(),
            Ty::List(t) => format!("_rt.List({})", self.descriptor(t)),
            Ty::Map(k, v) => format!("_rt.Map({}, {})", self.descriptor(k), self.descriptor(v)),
            Ty::Option(t) => format!("_rt.Option({})", self.descriptor(t)),
            Ty::Adt(d, args) => {
                let mut parts = vec![self.adt(*d)];
                parts.extend(args.iter().map(|a| self.descriptor(a)));
                format!("_rt.Adt({})", parts.join(", "))
            }
            Ty::Param(i) => format!("_rt.Param({i})"),
            Ty::Never | Ty::Var(_) | Ty::Dynamic | Ty::Error => "_rt.Any".into(),
        }
    }

    /// `import support.tickets as _support_tickets` for each module referred to.
    pub fn import_lines(&self) -> Vec<String> {
        self.imports
            .iter()
            .map(|&m| {
                let name = &self.program.module(m).name;
                format!("import {name} as {}", module_alias(name))
            })
            .collect()
    }
}

pub fn type_var(generic: &str) -> String {
    format!("_{generic}")
}

/// Whether a value of this type may be represented by Python `None`, so wrapping it in
/// an `Option` needs `_rt.some`.
pub fn may_be_none(ty: &Ty) -> bool {
    if let Ty::Refined(t, _) = ty {
        return may_be_none(t);
    }
    matches!(
        ty,
        Ty::Unit | Ty::Option(_) | Ty::Param(_) | Ty::Dynamic | Ty::Var(_) | Ty::Error
    )
}

/// A Python string literal.
pub fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_names() {
        assert_eq!(ident("from"), "from_");
        assert_eq!(ident("len"), "len_");
        assert_eq!(attr("len"), "len");
        assert_eq!(ident("ticket"), "ticket");
    }

    #[test]
    fn escapes_strings() {
        assert_eq!(string("a\"b\\c\nd\u{1}é"), "\"a\\\"b\\\\c\\nd\\x01é\"");
    }
}
