//! Every `tests/attacks/*.wardscript` is an injection attempt and MUST be rejected by `ward check`.

mod common;

#[test]
fn attacks_are_rejected() {
    let accepted: Vec<_> = common::wardscript_files("tests/attacks")
        .into_iter()
        .filter_map(|file| {
            let out = common::ward(&["check", file.to_str().expect("utf-8 path")]);
            // Exit code 2 (internal error) doesn't count: the checker has to actually reject it.
            (out.status.code() != Some(common::EXIT_DIAGNOSTICS))
                .then(|| format!("{}\n{}", file.display(), common::render(&out)))
        })
        .collect();
    assert!(
        accepted.is_empty(),
        "attack programs not rejected:\n\n{}",
        accepted.join("\n\n")
    );
}
