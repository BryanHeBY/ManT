//! Declarations in the shared declaration scanner.
use super::*;

#[test]
fn complete_groups_keep_every_explicit_spelling_and_exact_ranges() {
    for value in [
        "-c --stdout --to-stdout",
        "-c or --stdout --to-stdout",
        "(-0, -1, -2, -9)",
        "(-0 -1 -2 -9)",
        "-### --long --other",
    ] {
        let expected: &[&str] = if value.contains("-0") {
            &["-0", "-1", "-2", "-9"]
        } else if value.contains('#') {
            &["--long", "--other"]
        } else {
            &["-c", "--stdout", "--to-stdout"]
        };
        let nodes = [strong(vec![text(value)])];
        assert_names(&nodes, &[], expected);
        assert!(option_head(&nodes, &[]).complete);
    }
    let nodes = [text("-c --stdout --stdout")];
    assert_names(&nodes, &[], &["-c", "--stdout", "--stdout"]);
}

#[test]
fn effective_combined_style_can_begin_or_finish_one_complete_name() {
    for nodes in [
        vec![
            strong(vec![text("--a")]),
            strong(vec![emphasis(vec![text("ll")])]),
        ],
        vec![
            strong(vec![emphasis(vec![text("--a")])]),
            strong(vec![text("ll")]),
        ],
    ] {
        assert_names(&nodes, &[], &["--all"]);
        assert!(option_head(&nodes, &[]).complete);
    }
}

#[test]
fn alias_separators_consume_whole_or_tokens_and_complete_punctuation_flags() {
    for value in ["-a or --ascii", "-a  or\t--ascii", "-a or\n--ascii"] {
        assert_names(&[strong(vec![text(value)])], &[], &["-a", "--ascii"]);
        assert!(option_head(&[text(value)], &[]).inferred_complete);
    }
    for value in ["-., --hidden", "-@, --hidden"] {
        assert_names(&[text(value)], &[], &["--hidden"]);
        assert!(option_head(&[text(value)], &[]).inferred_complete);
    }
    for value in [
        "-a ordinary --fake",
        "-a orordinary --fake",
        "-a orphan --fake",
    ] {
        assert_names(&[text(value)], &[], &["-a"]);
        assert!(!option_head(&[text(value)], &[]).inferred_complete);
    }
}
