// clippy.toml only relaxes these inside #[test] fns, not in shared helpers.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;

use ward_syntax::ast::{ExprId, ExprKind, FnBody, Item, Lit, ModelValue, TemplatePart};
use ward_syntax::lexer::{TokenKind as T, lex};
use ward_syntax::printer::{print, print_expr_parenthesized};
use ward_syntax::{Parse, parse};

fn parse_ok(src: &str) -> Parse {
    let parse = parse(src);
    assert!(
        parse.diagnostics.is_empty(),
        "unexpected diagnostics for:\n{src}\n{:#?}",
        parse.diagnostics
    );
    parse
}

fn tail_expr(parse: &Parse) -> ExprId {
    match parse.module.items.first() {
        Some(Item::Fn(f)) => match &f.body {
            FnBody::Block(b) => b.tail.expect("block has a tail expression"),
            FnBody::Ai { .. } => panic!("expected a block body"),
        },
        _ => panic!("expected a function"),
    }
}

/// Parses `expr` as the value of a function body and prints it fully parenthesized.
fn parens(expr: &str) -> String {
    let parse = parse_ok(&format!("fn f() {{ {expr} }}"));
    print_expr_parenthesized(&parse.module, tail_expr(&parse))
}

fn codes(src: &str) -> Vec<&'static str> {
    parse(src).diagnostics.iter().map(|d| d.code.0).collect()
}

/// Printing is a fixed point: parse(print(ast)) prints the same text, with no errors.
fn assert_round_trips(src: &str) -> String {
    let printed = print(&parse_ok(src).module);
    let reprinted = print(&parse_ok(&printed).module);
    assert_eq!(printed, reprinted, "printer output is not stable");
    printed
}

#[test]
fn lexes_keywords_operators_and_literals() {
    let mut diags = Vec::new();
    let kinds: Vec<_> = lex(
        "ai fn llm x_1 _ 42 1_000 3.14 \"hi\" -> => <= && // c\n!",
        0,
        &mut diags,
    )
    .into_iter()
    .map(|t| t.kind)
    .collect();
    assert!(diags.is_empty());
    assert_eq!(
        kinds,
        [
            T::Ai,
            T::Fn,
            T::Ident,
            T::Ident,
            T::Underscore,
            T::Int,
            T::Int,
            T::Float,
            T::Str,
            T::Arrow,
            T::FatArrow,
            T::LtEq,
            T::AndAnd,
            T::Bang,
            T::Eof
        ]
    );
}

#[test]
fn string_with_escaped_quote_is_one_token() {
    let mut diags = Vec::new();
    let tokens = lex(r#""say \"hi\"" x"#, 0, &mut diags);
    assert!(diags.is_empty());
    assert_eq!(tokens[0].kind, T::Str);
    assert_eq!(tokens[1].kind, T::Ident);
}

#[test]
fn support_example_parses_and_prints() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/support.wardscript");
    let src = std::fs::read_to_string(path).expect("read example");
    let printed = assert_round_trips(&src);
    insta::assert_snapshot!(printed);
}

#[test]
fn all_examples_and_valid_ui_programs_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut checked = 0;
    for dir in ["examples", "tests/ui"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("read dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| e != "wardscript") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read file");
            if parse(&src).diagnostics.is_empty() {
                assert_round_trips(&src);
                checked += 1;
            }
        }
    }
    assert!(checked > 0);
}

#[test]
fn canonical_source_prints_unchanged() {
    let src = r#"import support.tickets as t
import mcp "gmail" as mail
@private(read_inbox)
@readonly(search)
import mcp "drive" as drive

pub type Page<T> {
    items: List<T>,
    next: Option<String>,
}

type Id = String

enum Shape {
    Point,
    Circle(Float),
}

pub ai fn classify(text: Untrusted<String>) -> Label
    uses {llm, net.read}
    budget {tokens: 500, cost: 0.01}
{
    "Label this: {text}"
}

ai fn echo(text: String) -> String {
    "Repeat: {text}"
}

fn area(s: Shape) -> Float {
    match s {
        Shape.Point => 0.0,
        Shape.Circle(r) => {
            let r2 = r * r
            3.14 * r2
        }
        _ => -1.0,
    }
}

@allow(rule_of_two, reason = "a human reviews every \"send\"")
@inline
fn loops(xs: List<Int>) -> Int
    uses {net.read}
{
    let total = 0
    for x in xs {
        if x > 0 && !skip(x) {
            total = total + x
        } else if x == 0 {
            continue_later()
        }
    }
    while total > 100 {
        total = total - 100
    }
    let p = Point { x: 1, y }
    if (Point { x: 1, y: 2 }) == p {
        return -total
    }
    total
}
"#;
    assert_eq!(print(&parse_ok(src).module), src);
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(parens("1 + 2 * 3"), "(1 + (2 * 3))");
    assert_eq!(parens("1 - 2 - 3"), "((1 - 2) - 3)");
    assert_eq!(parens("a || b && c"), "(a || (b && c))");
    assert_eq!(parens("a == b || c < d + 1"), "((a == b) || (c < (d + 1)))");
    assert_eq!(parens("-a.b(c)?"), "(-a.b(c)?)");
    assert_eq!(parens("!!x"), "(!(!x))");
    assert_eq!(parens("(1 + 2) * 3"), "((1 + 2) * 3)");
    assert_eq!(parens("xs[i + 1].name"), "xs[(i + 1)].name");
    assert_eq!(parens("a % b / c * d"), "(((a % b) / c) * d)");
}

#[test]
fn printer_adds_only_needed_parens() {
    let src = "fn f() {\n    (a + b) * (c - d) - (e - f)\n}\n";
    assert_eq!(assert_round_trips(src), src);
    // A statement can't start with `if`/`match` unless that's the whole statement.
    let src = "fn f() {\n    (if a {\n        1\n    } else {\n        2\n    } + 1)\n}\n";
    assert_eq!(assert_round_trips(src), src);
}

#[test]
fn templates_split_into_parts() {
    let parse = parse_ok(r#"fn f() { "Hi {user.name}, you owe {a + b}{{!}}" }"#);
    let ExprKind::Template(parts) = &parse.module.exprs[tail_expr(&parse)].kind else {
        panic!("expected a template");
    };
    let rendered: Vec<String> = parts
        .iter()
        .map(|p| match p {
            TemplatePart::Lit(s) => format!("lit:{s}"),
            TemplatePart::Expr(e) => {
                format!("expr:{}", print_expr_parenthesized(&parse.module, *e))
            }
        })
        .collect();
    assert_eq!(
        rendered,
        [
            "lit:Hi ",
            "expr:user.name",
            "lit:, you owe ",
            "expr:(a + b)",
            "lit:{!}"
        ]
    );
}

#[test]
fn plain_strings_decode_escapes() {
    let parse = parse_ok(r#"fn f() { "a\n\t\"b\"\\" }"#);
    assert_eq!(
        parse.module.exprs[tail_expr(&parse)].kind,
        ExprKind::Lit(Lit::Str("a\n\t\"b\"\\".into()))
    );
}

#[test]
fn record_literals_are_not_parsed_in_conditions() {
    // `x {` here starts the `if` body, not a record literal.
    assert_eq!(
        parens("if x { 1 } else { 2 }").lines().next(),
        Some("if x {")
    );
    assert!(codes("fn f() { if x { 1 } }").is_empty());
}

#[test]
fn recovers_and_reports_every_error() {
    let src = "
fn a() {
    let x = 1 2
    let y = ;
    let z = 3;
}

fn b(x: ) -> Int { x }

let stray = 1;

fn c() -> Int { 1 < 2 < 3 }
";
    assert_eq!(codes(src), ["W0015", "W0012", "W0013", "W0011", "W0019"]);
}

#[test]
fn unclosed_block_reports_the_opening_brace() {
    let parse = parse("fn a() {\n    let x = 1;\n\nfn b() {}\n");
    assert_eq!(parse.diagnostics.len(), 1);
    assert_eq!(parse.diagnostics[0].code.0, "W0021");
    assert_eq!(parse.diagnostics[0].span().start, 7);
    // The following item is still parsed.
    assert_eq!(parse.module.items.len(), 2);
}

#[test]
fn one_bad_token_reports_once() {
    assert_eq!(codes("fn f() { g(1 2 3); }").len(), 1);
}

#[test]
fn doubled_braces_are_literal() {
    let parse = parse_ok(r#"fn f() { "{{\"a\": {x}}}" }"#);
    let printed = print(&parse.module);
    assert!(printed.contains(r#""{{\"a\": {x}}}""#), "{printed}");
    let ExprKind::Template(parts) = &parse.module.exprs[tail_expr(&parse)].kind else {
        panic!("expected a template");
    };
    assert!(matches!(&parts[0], TemplatePart::Lit(s) if s == "{\"a\": "));
    assert!(matches!(&parts[2], TemplatePart::Lit(s) if s == "}"));
}

#[test]
fn record_literals_may_name_a_module_type() {
    let src = "fn f() {\n    tickets.Ticket { title: \"x\" }\n}\n";
    assert_eq!(assert_round_trips(src), src);
    // Field access followed by a block is still field access in conditions.
    assert!(codes("fn f() { if t.ok { 1 } else { 2 } }").is_empty());
}

fn stmt_count(src: &str) -> usize {
    let parse = parse_ok(src);
    match parse.module.items.first() {
        Some(Item::Fn(f)) => match &f.body {
            FnBody::Block(b) => b.stmts.len() + usize::from(b.tail.is_some()),
            FnBody::Ai { .. } => 0,
        },
        _ => panic!("expected a function"),
    }
}

#[test]
fn line_breaks_end_statements() {
    assert_eq!(stmt_count("fn f() {\n    let x = a\n    -b\n}"), 2);
    assert_eq!(stmt_count("fn f() {\n    g(x)\n    (y)\n}"), 2);
    assert_eq!(stmt_count("fn f() {\n    xs\n    [0]\n}"), 2);
    assert_eq!(
        stmt_count("fn f() {\n    let a = 1; let b = 2\n    a\n}"),
        3
    );
    // A line break doesn't split an operator from its right operand, a leading `.`,
    // or anything inside parentheses or brackets.
    assert_eq!(
        stmt_count("fn f() {\n    let x = a +\n        b\n    x\n}"),
        2
    );
    assert_eq!(
        stmt_count("fn f() {\n    text\n        .trim()\n        .lower()\n}"),
        1
    );
    assert_eq!(stmt_count("fn f() {\n    g(a,\n      b\n      + c)\n}"), 1);
    assert_eq!(
        stmt_count("fn f() {\n    let xs = [\n        1,\n        2,\n    ]\n    xs\n}"),
        2
    );
}

#[test]
fn line_break_before_brace_is_not_a_record_literal() {
    // `x` then a block statement, not `x { ... }`.
    assert_eq!(stmt_count("fn f() {\n    x\n    {\n        1\n    }\n}"), 2);
}

#[test]
fn match_arms_may_be_separated_by_line_breaks() {
    let src = "fn f(x: Int) -> Int {\n    match x {\n        0 => 1\n        _ => 2\n    }\n}";
    assert!(codes(src).is_empty());
}

#[test]
fn two_statements_on_one_line_need_a_semicolon() {
    assert_eq!(codes("fn f() {\n    let x = 1 let y = 2\n}"), ["W0015"]);
}

#[test]
fn annotations() {
    let parse = parse_ok("@allow(rule_of_two, reason = \"why\")\npub fn f() {}\n");
    let Some(Item::Fn(f)) = parse.module.items.first() else {
        panic!("expected a function");
    };
    assert!(f.is_pub);
    let [a] = f.annotations.as_slice() else {
        panic!("expected one annotation");
    };
    assert_eq!(a.name.name, "allow");
    assert_eq!(a.args.len(), 2);
    assert_eq!(a.args[0].name.name, "rule_of_two");
    assert!(a.args[0].value.is_none());
    assert_eq!(
        a.args[1].value.as_ref().map(|(v, _)| v.as_str()),
        Some("why")
    );

    let parse = parse_ok("@not_sink(send.body, reason = \"why\")\nimport mcp \"gmail\" as mail\n");
    let Some(Item::Import(i)) = parse.module.items.first() else {
        panic!("expected an import");
    };
    assert_eq!(i.annotations[0].args[0].name.name, "send.body");
    assert_eq!(codes("@sink(send.)\nimport mcp \"g\" as g\n"), ["W0010"]);

    assert_eq!(codes("@x\ntype T = Int\n"), ["W0024"]);
    assert_eq!(codes("@x\nenum E { A }\n"), ["W0024"]);
    assert_eq!(codes("@allow(a = b)\nfn f() {}\n"), ["W0010"]);
    assert_eq!(codes("@allow(a = \"{x}\")\nfn f() {}\n"), ["W0022"]);
    assert_eq!(codes("@\nfn f() {}\n"), ["W0010"]);
}

#[test]
fn model_clause() {
    let src = "ai fn f(x: String) -> String\n    budget {calls: 4}\n    model {primary: fast, fallback: [smart, backup], retries: 2, backoff: 0.5}\n{\n    \"{x}\"\n}\n";
    let parse = parse_ok(src);
    let Some(Item::Fn(f)) = parse.module.items.first() else {
        panic!("expected a function");
    };
    let Some(model) = &f.model else {
        panic!("expected a model clause");
    };
    let names: Vec<&str> = model.entries.iter().map(|e| e.name.name.as_str()).collect();
    assert_eq!(names, ["primary", "fallback", "retries", "backoff"]);
    assert!(matches!(&model.entries[1].value, ModelValue::Names(v, _) if v.len() == 2));
    assert!(matches!(&model.entries[3].value, ModelValue::Number(n, _) if n == "0.5"));
    assert_eq!(assert_round_trips(src), src);

    // `model` is still a name everywhere else.
    parse_ok("fn model(model: Int) -> Int {\n    let model = model\n    model\n}\n");
    assert_eq!(
        codes(
            "ai fn f() -> Int\n    model {primary: fast}\n    model {retries: 1}\n{\n    \"x\"\n}\n"
        ),
        ["W0018"]
    );
    assert_eq!(
        codes("ai fn f() -> Int\n    model {primary: \"fast\"}\n{\n    \"x\"\n}\n"),
        ["W0010"]
    );
    assert_eq!(
        codes("ai fn f() -> Int\n    model {primary fast}\n{\n    \"x\"\n}\n"),
        ["W0010"]
    );
}
