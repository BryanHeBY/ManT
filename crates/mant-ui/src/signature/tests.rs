use super::*;

fn role(value: &str, accents: &[Accent], needle: &str) -> Option<Role> {
    let byte = value.find(needle).unwrap();
    accents
        .iter()
        .find(|accent| accent.bytes.contains(&byte))
        .map(|accent| accent.role)
}

fn scan(value: &str, context: Context) -> Vec<Accent> {
    let result = accents(value, context, &[], &[]);
    for accent in &result {
        assert!(value.is_char_boundary(accent.bytes.start));
        assert!(value.is_char_boundary(accent.bytes.end));
        assert!(accent.bytes.start < accent.bytes.end && accent.bytes.end <= value.len());
    }
    assert!(
        result
            .windows(2)
            .all(|pair| pair[0].bytes.end <= pair[1].bytes.start)
    );
    result
}

#[test]
fn plain_paragraph_usages_keep_distinct_command_parameter_and_structure_roles() {
    for value in [
        "lua [ options ] [ script [ args ] ]",
        "curl [options / URLs]",
        "systemctl [OPTIONS...] COMMAND [UNIT...]",
        "bash [options] [command_string | file]",
        "ls [OPTION]... [FILE]...",
        "make [OPTION]... [TARGET]...",
    ] {
        let result = scan(value, Context::Synopsis);
        assert_eq!(
            role(value, &result, value.split_whitespace().next().unwrap()),
            Some(Role::Command)
        );
        assert_eq!(role(value, &result, "["), Some(Role::Delimiter));
        assert!(
            result.iter().any(|accent| accent.role == Role::Placeholder),
            "{value}"
        );
    }
    let value = "lua [ options ] [ script [ args ] ]";
    let result = scan(value, Context::Synopsis);
    for needle in ["options", "script", "args"] {
        assert_eq!(role(value, &result, needle), Some(Role::Placeholder));
    }
    assert_eq!(scan(value, Context::Inferred), []);
}

#[test]
fn strict_usage_inference_does_not_reclassify_ordinary_shell_arguments() {
    let value = "sed [-l N] [--line-length=N] [-e script] [--expression=script]\n    [script-if-no-other-script] [file...]";
    let result = scan(value, Context::Inferred);
    for needle in ["N", "script", "script-if-no-other-script", "file"] {
        assert_eq!(
            role(value, &result, needle),
            Some(Role::Placeholder),
            "{needle}"
        );
    }
    assert_eq!(role(value, &result, "--expression"), Some(Role::Option));
    assert_eq!(role(value, &result, "="), Some(Role::Operator));
    assert_eq!(role(value, &result, "-no-other"), Some(Role::Placeholder));
    for value in [
        "curl --output file",
        "echo file",
        "echo [ -f file ]",
        "[[ -f file ]]",
        "cat < input | grep file",
        "tool [--arg $(echo file)]",
        "tool [--arg $HOME]",
        "tool [--arg || --other]",
        "struct S { int field[N]; };",
        "int regexec(regex_t *value, int flags);",
        "#include <regex.h>",
    ] {
        assert_eq!(scan(value, Context::Inferred), [], "{value}");
    }
    let value = "\tgit [-v]\n    <mixed-indent>";
    let result = scan(value, Context::Inferred);
    assert_eq!(role(value, &result, "mixed-indent"), None);
    let value = "\tgit [-v]\n\t    <same-prefix>";
    let result = scan(value, Context::Inferred);
    assert_eq!(role(value, &result, "same-prefix"), Some(Role::Placeholder));
}

#[test]
fn fonts_are_contextual_evidence_and_the_first_command_wins_over_italics() {
    let value = "git [-C <path>] <command> [<args>]";
    let result = accents(value, Context::Synopsis, std::slice::from_ref(&(0..3)), &[]);
    assert_eq!(role(value, &result, "git"), Some(Role::Command));
    assert_eq!(role(value, &result, "path"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "command"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "-C"), Some(Role::Option));
    let value = "ssh [-p port]\n    [-L [bind_address:]port] destination [command [argument...]]\nssh [-Q query_option]";
    let result = scan(value, Context::Synopsis);
    for needle in [
        "port",
        "bind_address",
        "destination",
        "argument",
        "query_option",
    ] {
        assert_eq!(
            role(value, &result, needle),
            Some(Role::Placeholder),
            "{needle}"
        );
    }
}

#[test]
fn validated_forms_protect_names_quotes_urls_and_fixed_choices() {
    let value = "--output[=<文件e\u{301}👩‍💻>] --expression=script [-i[SUFFIX]] --mode=[fast|slow] 'file' https://host/?q=[file]";
    let result = accents(value, Context::Entry, &[], std::slice::from_ref(&(0..8)));
    assert_eq!(role(value, &result, "--output"), None);
    assert_eq!(role(value, &result, "文件"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "script"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "SUFFIX"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "fast"), None);
    assert_eq!(role(value, &result, "slow"), None);
    assert_eq!(role(value, &result, "'file'"), None);
    assert_eq!(role(value, &result, "https"), None);
    let value = "operator< --value=<path>";
    let result = accents(value, Context::Entry, &[], std::slice::from_ref(&(0..9)));
    assert_eq!(role(value, &result, "operator"), None);
    assert_eq!(role(value, &result, "path"), Some(Role::Placeholder));
}

#[test]
fn emphasis_only_becomes_an_argument_inside_a_validated_usage_or_form() {
    let value = "-e statement";
    let result = accents(
        value,
        Context::Entry,
        std::slice::from_ref(&(3..12)),
        std::slice::from_ref(&(0..2)),
    );
    assert_eq!(role(value, &result, "-e"), None);
    assert_eq!(role(value, &result, "statement"), Some(Role::Placeholder));
    let value = "This is explanatory text";
    assert_eq!(
        accents(
            value,
            Context::Synopsis,
            std::slice::from_ref(&(8..19)),
            &[]
        ),
        []
    );
    assert_eq!(
        accents(
            value,
            Context::Inferred,
            std::slice::from_ref(&(8..19)),
            &[]
        ),
        []
    );

    let value = "tool [operand]";
    let start = value.find("operand").unwrap();
    for split in 0..=7 {
        let emphasis: Vec<_> = [start..start + split, start + split..start + 7]
            .into_iter()
            .filter(|range| !range.is_empty())
            .collect();
        let result = accents(value, Context::Synopsis, &emphasis, &[]);
        assert_eq!(role(value, &result, "operand"), Some(Role::Placeholder));
    }
    assert_eq!(
        scan("tool [-x file] is explanatory prose", Context::Synopsis),
        []
    );
}

#[test]
fn option_spelling_suffixes_are_distinct_from_optional_uppercase_arguments() {
    let value = "ip [-V[ersion]] [-i[SUFFIX]] [--color=WHEN]";
    let start = value.find("ersion").unwrap();
    let result = accents(
        value,
        Context::Synopsis,
        std::slice::from_ref(&(start..start + 6)),
        &[],
    );
    assert_eq!(role(value, &result, "ersion"), None);
    assert_eq!(role(value, &result, "SUFFIX"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "WHEN"), Some(Role::Placeholder));
    let value = "OBJECT := { address | link | route }";
    let result = scan(value, Context::Synopsis);
    assert_eq!(role(value, &result, "OBJECT"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, ":="), Some(Role::Operator));
    assert_eq!(role(value, &result, "address"), None);
}

#[test]
fn malformed_or_excessive_owners_fall_back_without_partial_ranges() {
    for value in [
        "tool [-x <file]",
        "tool [-x 'file]",
        "tool [-x [file]",
        "tool [-x file]]",
    ] {
        assert_eq!(scan(value, Context::Synopsis), [], "{value}");
    }
    let value = format!(
        "tool {}file{}",
        "[".repeat(MAX_DEPTH + 1),
        "]".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(scan(&value, Context::Synopsis), []);
    let value = format!("tool [-x {}]", "x".repeat(MAX_BYTES));
    assert_eq!(scan(&value, Context::Synopsis), []);
    assert_eq!(
        accents("é", Context::Entry, std::slice::from_ref(&(0..1)), &[]),
        []
    );
    assert_eq!(
        accents("word", Context::Entry, &[], std::slice::from_ref(&(0..5))),
        []
    );
}

#[test]
fn repeated_compact_groups_keep_bounded_work_and_valid_coordinates() {
    // No whitespace between groups: URL lookahead and group evidence must
    // stop locally, rather than search the whole remaining owner per token.
    let value = format!("tool {}", "[file]".repeat(8_000));
    let result = scan(&value, Context::Synopsis);
    assert_eq!(role(&value, &result, "file"), Some(Role::Placeholder));
    assert_eq!(scan(&value, Context::Inferred), []);
}

#[test]
fn urls_preserve_balanced_internal_brackets_and_leave_outer_groups_complete() {
    for value in [
        "tool [--url=https://example.org] [-f FILE]",
        "tool [-f FILE] [--url=https://example.org]",
        "tool [--url=https://[::1]/?q=[x]] [-f FILE]",
        "tool [-f FILE] [--url=https://[::1]/?q=[x]]",
        "tool [--url=<https://[::1]/?q=[x]>] [-f FILE]",
    ] {
        let result = scan(value, Context::Inferred);
        assert_eq!(role(value, &result, "tool"), Some(Role::Command), "{value}");
        assert_eq!(role(value, &result, "https"), None, "{value}");
        assert_eq!(
            role(value, &result, "FILE"),
            Some(Role::Placeholder),
            "{value}"
        );
        if value.contains("::1") {
            assert_eq!(role(value, &result, "[::1]"), None, "{value}");
            assert_eq!(role(value, &result, "[x]"), None, "{value}");
        }
    }
    for value in [
        "--url=(https://example.org/a(b)) --file=<FILE>",
        "--url={https://example.org/a{b}} --file=<FILE>",
        "--url=https://example.org/?q=[x] --file=<FILE>",
    ] {
        let result = scan(value, Context::Entry);
        assert_eq!(role(value, &result, "https"), None, "{value}");
        assert_eq!(
            role(value, &result, "FILE"),
            Some(Role::Placeholder),
            "{value}"
        );
    }
}

#[test]
fn source_fonts_cannot_establish_a_command_without_structural_usage_evidence() {
    for value in [
        "size_t count",
        "sig_atomic_t flag",
        "see options",
        "program file",
    ] {
        for emphasis in [vec![], std::iter::once(0..value.len()).collect::<Vec<_>>()] {
            assert_eq!(
                accents(value, Context::Synopsis, &emphasis, &[]),
                [],
                "{value}"
            );
        }
    }
    let value = "program [-f file]";
    assert_eq!(
        role(
            value,
            &accents(
                value,
                Context::Synopsis,
                std::slice::from_ref(&(0..value.len())),
                &[]
            ),
            "program"
        ),
        Some(Role::Command)
    );
}

#[test]
fn uppercase_choice_literals_do_not_become_parameters_without_extra_evidence() {
    let value = "bash [options] [command_string | file]";
    let result = scan(value, Context::Synopsis);
    for parameter in ["command_string", "file"] {
        assert_eq!(role(value, &result, parameter), Some(Role::Placeholder));
    }
    for (value, literals) in [
        ("tar {A|c|d|r|t|u|x} [FILE...]", vec!["A", "c", "x}"]),
        ("tool [--method=[GET|POST]]", vec!["GET", "POST"]),
        (
            "tool [--kind={CUSTOM_KIND|OTHER_KIND}]",
            vec!["CUSTOM_KIND", "OTHER_KIND"],
        ),
    ] {
        let result = scan(value, Context::Synopsis);
        for literal in literals {
            assert_eq!(role(value, &result, literal), None, "{value}: {literal}");
        }
    }
    let value = "tool [FILE|DIRECTORY]";
    let result = scan(value, Context::Synopsis);
    for parameter in ["FILE", "DIRECTORY"] {
        assert_eq!(role(value, &result, parameter), Some(Role::Placeholder));
    }
    let value = "tool [CUSTOM_KIND|OTHER_KIND]";
    let start = value.find("CUSTOM_KIND").unwrap();
    let result = accents(
        value,
        Context::Synopsis,
        std::slice::from_ref(&(start..start + 11)),
        &[],
    );
    assert_eq!(role(value, &result, "CUSTOM_KIND"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "OTHER_KIND"), None);
    let value = "--method=[GET|POST]";
    let result = scan(value, Context::Entry);
    assert_eq!(role(value, &result, "GET"), None);
    assert_eq!(role(value, &result, "POST"), None);
}

#[test]
fn concrete_numbers_keep_their_literal_roles_except_inside_explicit_placeholders() {
    let value = "tool [-n 3] [--limit=0x20] [-w 1.5] [-f <3>]";
    for context in [Context::Inferred, Context::Synopsis, Context::Entry] {
        let result = scan(value, context);
        for number in ["3]", "0x20", "1.5"] {
            assert_eq!(role(value, &result, number), None, "{context:?}: {number}");
        }
        assert_eq!(role(value, &result, "3>"), Some(Role::Placeholder));
    }
}
