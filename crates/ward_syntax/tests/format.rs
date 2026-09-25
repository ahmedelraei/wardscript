//! `format` keeps every comment, is idempotent, and doesn't change the program.

#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;

use ward_syntax::parse;
use ward_syntax::printer::{format, print};

fn sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut dirs = vec![root.join("examples"), root.join("tests")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).expect("read dir") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                dirs.push(p);
            } else if p
                .extension()
                .is_some_and(|x| x == "ward" || x == "wardscript")
            {
                out.push((
                    p.display().to_string(),
                    std::fs::read_to_string(&p).expect("read"),
                ));
            }
        }
    }
    out.sort();
    out
}

fn comments(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|l| l.find("//").map(|i| l[i..].trim_end().to_owned()))
        .collect()
}

#[test]
fn formats_every_program_faithfully() {
    let mut formatted_count = 0;
    for (path, src) in sources() {
        let Ok(out) = format(&src) else {
            continue; // Files with syntax errors aren't formatted.
        };
        formatted_count += 1;
        assert_eq!(
            format(&out).as_deref(),
            Ok(out.as_str()),
            "{path}: not idempotent"
        );
        let before = print(&parse(&src).module);
        let after = print(&parse(&out).module);
        assert_eq!(before, after, "{path}: formatting changed the program");
        let mut want = comments(&src);
        let mut got = comments(&out);
        // Strings containing `//` (URLs) show up in both; compare as multisets.
        want.sort();
        got.sort();
        assert_eq!(want, got, "{path}: comments changed\n{out}");
    }
    assert!(
        formatted_count > 50,
        "only {formatted_count} files formatted"
    );
}

#[test]
fn keeps_comments_in_place() {
    let src = "// A module comment.\n\n// About f.\nfn f(x: Int) -> Int {   // trailing on the header\n    // before a\n    let a = x   // after a\n\n    // after a blank line\n    let b = a + 1\n    b\n    // at the end\n}\n\ntype T {\n    // a field\n    x: Int,   // trailing field\n}\n\nfn g(x: Int) -> Int {\n    match x {\n        // zero\n        0 => 1,\n        _ => 2, // other\n    }\n}\n";
    let out = format(src).expect("formats");
    let expected = "// A module comment.\n\n// About f.\nfn f(x: Int) -> Int {  // trailing on the header\n    // before a\n    let a = x  // after a\n\n    // after a blank line\n    let b = a + 1\n    b\n    // at the end\n}\n\ntype T {\n    // a field\n    x: Int,  // trailing field\n}\n\nfn g(x: Int) -> Int {\n    match x {\n        // zero\n        0 => 1,\n        _ => 2,  // other\n    }\n}\n";
    assert_eq!(out, expected);
    assert!(format("fn f( {").is_err());
}
