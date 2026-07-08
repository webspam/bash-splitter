//! Golden snapshots of the full output JSON in both modes, managed by insta:
//! `.snap` files live in `tests/snapshots/`. After an intentional format change,
//! refresh them with `cargo insta review` (or `INSTA_UPDATE=always cargo test`).

use bash_splitter::{split, split_nested};

/// One test per case, snapshotting `NAME.flat` and `NAME.nested`.
macro_rules! snapshot_tests {
    ($($name:ident: $input:expr,)*) => {$(
        #[test]
        fn $name() {
            let input = $input;
            insta::assert_json_snapshot!(
                concat!(stringify!($name), ".flat"),
                split(input, true).expect("parse")
            );
            insta::assert_json_snapshot!(
                concat!(stringify!($name), ".nested"),
                split_nested(input, true).expect("parse")
            );
        }
    )*};
}

snapshot_tests! {
    simple_command: "ls -la",
    pipeline: "grep foo | sort -u | wc -l",
    sequence: r#"echo "start" && echo "middle"; echo "end""#,
    single_substitution: r#"cd "$(echo pie)""#,
    nested_substitutions: "echo $(foo $(bar))",
    multiple_substitutions: r#"echo "$(whoami)" "$(hostname)""#,
    loop_with_redirect_and_vars: r#"for f in *.txt; do grep "$pattern" "$f" > "out/$f"; done"#,
    loop_with_nested_substitution: r#"for f in *.txt; do grep "$(cat pattern.txt)" "$f" > "out/$f"; done"#,
    orphan_substitution: "[[ -n $(whoami) ]]",
    complex_pipeline_with_subs: r#"cd "$(echo pie)" && cat "$(grep x)" | sort -u"#,
    static_for: r#"for f in a b; do grep "$f" log | head -1; done"#,
    while_loop: "seq 3 | while read -r x; do echo $x; done",
}
