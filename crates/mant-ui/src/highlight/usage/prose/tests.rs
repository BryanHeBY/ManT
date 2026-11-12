use super::*;

fn all(value: &str) -> Vec<Range<usize>> {
    std::iter::once(0..value.len()).collect()
}

fn range(value: &str, needle: &str) -> Range<usize> {
    let start = value.find(needle).unwrap();
    start..start + needle.len()
}

fn role(value: &str, accents: &[Accent], needle: &str) -> Option<Role> {
    let start = value.find(needle).unwrap();
    accents
        .iter()
        .find(|accent| accent.bytes.contains(&start))
        .map(|accent| accent.role)
}

fn checked(
    value: &str,
    marked: &[Range<usize>],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
) -> Vec<Accent> {
    let result = accents(value, marked, emphasis, protected);
    for accent in &result {
        assert!(accent.bytes.start < accent.bytes.end && accent.bytes.end <= value.len());
        assert!(value.is_char_boundary(accent.bytes.start));
        assert!(value.is_char_boundary(accent.bytes.end));
    }
    assert!(
        result
            .windows(2)
            .all(|pair| pair[0].bytes.end <= pair[1].bytes.start)
    );
    result
}

#[test]
fn gcc_inventories_need_no_command_head_and_retain_special_option_names() {
    let value = "-c -S -E -o file -### -nostdinc++ -dumpbase-ext auxdropsuf @file -fplugin-arg-name=arg -ffile-prefix-map=old=new";
    let emphasis = ["file", "auxdropsuf", "name", "old", "new"]
        .into_iter()
        .map(|word| range(value, word))
        .chain(std::iter::once({
            let start = value.find("=arg").unwrap() + 1;
            start..start + 3
        }))
        .collect::<Vec<_>>();
    let mut emphasis = emphasis;
    emphasis.sort_by_key(|range| range.start);
    let result = checked(value, &all(value), &emphasis, &[]);
    for option in [
        "-c",
        "-S",
        "-E",
        "-o",
        "-###",
        "-nostdinc++",
        "-dumpbase-ext",
        "-fplugin-arg-",
        "-ffile-prefix-map",
    ] {
        assert_eq!(role(value, &result, option), Some(Role::Option), "{option}");
    }
    for word in ["file", "auxdropsuf", "name", "old", "new"] {
        assert_eq!(
            role(value, &result, word),
            Some(Role::Placeholder),
            "{word}"
        );
    }
    assert_eq!(role(value, &result, "@"), Some(Role::Operator));
    assert_eq!(role(value, &result, "="), Some(Role::Operator));
}

#[test]
fn prose_colors_only_complete_marked_options_not_font_boundary_suffixes() {
    let value = "Use --help or -O2. machine-gcc and /tmp/-x are names; --plain is unmarked.";
    let marked = ["--help", "-O2", "-gcc", "-x"]
        .into_iter()
        .map(|word| range(value, word))
        .collect::<Vec<_>>();
    let result = checked(value, &marked, &[], &[]);
    assert_eq!(role(value, &result, "--help"), Some(Role::Option));
    assert_eq!(role(value, &result, "-O2"), Some(Role::Option));
    for word in ["Use", "machine-gcc", "-gcc", "/tmp/-x", "-x", "--plain"] {
        assert_eq!(role(value, &result, word), None, "{word}");
    }
}

#[test]
fn whole_italic_option_and_partial_italic_template_have_distinct_roles() {
    let value = "Use --parallel or -fplugin-arg-name=arg; add --output file.";
    let marked = ["--parallel", "-fplugin-arg-name=arg", "--output file"]
        .into_iter()
        .map(|word| range(value, word))
        .collect::<Vec<_>>();
    let emphasis = ["--parallel", "name", "arg", "file"]
        .into_iter()
        .map(|word| {
            // The argument after '=' is distinct from '-arg-' in the option.
            if word == "arg" {
                let start = value.find("=arg").unwrap() + 1;
                start..start + 3
            } else {
                range(value, word)
            }
        })
        .collect::<Vec<_>>();
    let result = checked(value, &marked, &emphasis, &[]);
    assert_eq!(role(value, &result, "--parallel"), Some(Role::Option));
    assert_eq!(role(value, &result, "-fplugin-arg-"), Some(Role::Option));
    assert_eq!(role(value, &result, "name"), Some(Role::Placeholder));
    let argument = value.find("=arg").unwrap() + 1;
    assert!(
        result
            .iter()
            .any(|accent| accent.bytes.contains(&argument) && accent.role == Role::Placeholder)
    );
    assert_eq!(role(value, &result, "file"), Some(Role::Placeholder));
}

#[test]
fn fixed_choices_spelling_suffixes_and_concrete_numbers_stay_conservative() {
    let value =
        "-V[ersion] --method=[GET|POST] --color=[always|never] -i[SUFFIX] --count=3 -n 0x20";
    let emphasized = range(value, "ersion");
    let emphasis = std::slice::from_ref(&emphasized);
    let result = checked(value, &all(value), emphasis, &[]);
    for word in ["ersion", "GET", "POST", "always", "never", "3", "0x20"] {
        assert_eq!(role(value, &result, word), None, "{word}");
    }
    assert_eq!(role(value, &result, "SUFFIX"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "["), Some(Role::Delimiter));
    assert_eq!(role(value, &result, "="), Some(Role::Operator));
}

#[test]
fn malformed_local_atoms_do_not_drop_independent_references() {
    for value in [
        "Try --first ??? --second.",
        "Try --first [broken --second.",
        "Try --first mismatched> --second.",
    ] {
        let marked = [range(value, "--first"), range(value, "--second")];
        let result = checked(value, &marked, &[], &[]);
        for option in ["--first", "--second"] {
            assert_eq!(role(value, &result, option), Some(Role::Option), "{value}");
        }
    }
    let value = "-c ??? -S";
    let result = checked(value, &[], &[], &[]);
    assert_eq!(role(value, &result, "-c"), Some(Role::Option));
    assert_eq!(role(value, &result, "-S"), Some(Role::Option));
}

#[test]
fn quoted_uri_numeric_and_protected_tokens_do_not_gain_option_colors() {
    let value = "Use \"--quoted\" and https://host/-url?q=[-z] plus -1 -20; --name=文件 remains.";
    let result = checked(
        value,
        &all(value),
        std::slice::from_ref(&range(value, "文件")),
        std::slice::from_ref(&range(value, "--name")),
    );
    for word in ["--quoted", "-url", "-z", "-1", "-20", "--name"] {
        assert_eq!(role(value, &result, word), None, "{word}");
    }
    assert_eq!(role(value, &result, "文件"), None);
}

#[test]
fn invalid_ranges_and_excessive_resources_reject_the_whole_owner() {
    let value = "--alpha --beta";
    assert_eq!(
        accents(value, std::slice::from_ref(&(0..100)), &[], &[]),
        []
    );
    assert_eq!(
        accents("é --help", std::slice::from_ref(&(0..1)), &[], &[]),
        []
    );
    let value = format!(
        "--alpha {}x{} --beta",
        "[".repeat(MAX_DEPTH + 1),
        "]".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(accents(&value, &all(&value), &[], &[]), []);
    let value = format!("--alpha {} --beta", "x".repeat(MAX_BYTES));
    assert_eq!(accents(&value, &all(&value), &[], &[]), []);
}

#[test]
fn source_work_is_linear_for_long_scheme_runs_and_many_font_fragments() {
    for count in [500, 1_000, 2_000] {
        let value = format!("Use {} --help.", "a...".repeat(count));
        let mut work = Work::default();
        let result = accents_with_work(&value, &all(&value), &[], &[], &mut work).unwrap();
        assert_eq!(role(&value, &result, "--help"), Some(Role::Option));
        assert!(
            work.steps <= value.len() * 12,
            "{} steps for {} bytes",
            work.steps,
            value.len()
        );

        let value = format!("-f{} --help", "a".repeat(count * 4));
        let emphasis: Vec<_> = (3..count * 4)
            .step_by(2)
            .map(|start| start..start + 1)
            .collect();
        let mut work = Work::default();
        let result = accents_with_work(&value, &all(&value), &emphasis, &[], &mut work).unwrap();
        assert_eq!(role(&value, &result, "--help"), Some(Role::Option));
        assert!(
            work.steps <= value.len() * 20,
            "{} steps for {} bytes",
            work.steps,
            value.len()
        );
    }
}

#[test]
fn many_tiny_marked_options_do_not_rescan_all_prior_ranges() {
    for count in [500, 1_000, 2_000] {
        let value = "Use -a ".repeat(count);
        let marked: Vec<_> = value
            .match_indices("-a")
            .map(|(start, _)| start..start + 2)
            .collect();
        let mut work = Work::default();
        let result = accents_with_work(&value, &marked, &[], &[], &mut work).unwrap();
        assert_eq!(result.len(), count);
        assert!(
            work.steps <= value.len() * 12 + marked.len() * 4,
            "{} steps for {} bytes",
            work.steps,
            value.len()
        );
    }
}

#[test]
fn independent_shell_tests_are_not_option_inventories() {
    for value in [
        "[ -x file | -y file ]",
        "[[ -x file || -y file ]]",
        "[\t-x file | -y file ]",
    ] {
        assert_eq!(accents(value, &[], &[], &[]), [], "{value}");
        let result = accents(value, &all(value), &[], &[]);
        assert_eq!(
            role(value, &result, "|"),
            None,
            "shell operator belongs to code scanner"
        );
    }
    let value = "[-x file | -y file]";
    assert_eq!(
        role(value, &accents(value, &[], &[], &[]), "|"),
        Some(Role::Delimiter)
    );
}

#[test]
fn optional_assignment_groups_differ_from_option_spelling_suffixes() {
    let value = "--help[=class[,...]] -V[ersion] -i[SUFFIX]";
    let emphasis = [range(value, "class"), range(value, "ersion")];
    let result = checked(value, &all(value), &emphasis, &[]);
    assert_eq!(role(value, &result, "class"), Some(Role::Placeholder));
    assert_eq!(role(value, &result, "ersion"), None);
    assert_eq!(role(value, &result, "SUFFIX"), Some(Role::Placeholder));
}

#[test]
fn unicode_option_parameters_require_original_emphasis_evidence() {
    let name = "文件e\u{301}👩‍💻";
    let value = format!("-fplugin-arg-{name}=arg -c -S");
    let result = checked(
        &value,
        &all(&value),
        std::slice::from_ref(&range(&value, name)),
        &[],
    );
    assert_eq!(role(&value, &result, "-fplugin-arg-"), Some(Role::Option));
    assert_eq!(role(&value, &result, name), Some(Role::Placeholder));
    assert_eq!(role(&value, &result, "-c"), Some(Role::Option));
    let result = checked(&value, &all(&value), &[], &[]);
    assert_eq!(
        role(&value, &result, name),
        None,
        "Unicode alone is not a template declaration"
    );
}

#[test]
fn arbitrary_emphasis_after_an_option_is_not_argument_arity_evidence() {
    let value = "Use --help instead.";
    let marked = [range(value, "--help"), range(value, "instead")];
    let result = checked(
        value,
        &marked,
        std::slice::from_ref(&range(value, "instead")),
        &[],
    );
    assert_eq!(role(value, &result, "--help"), Some(Role::Option));
    assert_eq!(role(value, &result, "instead"), None);
    let value = "Use --output file.";
    let result = checked(
        value,
        &all(value),
        std::slice::from_ref(&range(value, "file")),
        &[],
    );
    assert_eq!(role(value, &result, "file"), Some(Role::Placeholder));
}

#[test]
fn mathematical_protection_is_opaque_to_quotes_groups_and_option_evidence() {
    for math in [
        "'".to_owned(),
        "\"".to_owned(),
        "[".repeat(65),
        "$(".repeat(65),
    ] {
        let value = format!("{math} is followed by --help.");
        let result = checked(
            &value,
            std::slice::from_ref(&range(&value, "--help")),
            &[],
            std::slice::from_ref(&(0..math.len())),
        );
        assert_eq!(
            role(&value, &result, "--help"),
            Some(Role::Option),
            "{math}"
        );
        assert_eq!(result.len(), 1);
    }
    let value = "-f file";
    let result = checked(
        value,
        &all(value),
        std::slice::from_ref(&(3..7)),
        std::slice::from_ref(&(0..2)),
    );
    assert_eq!(
        result,
        [],
        "opaque math cannot pretend to be an option head"
    );
    let value = "--help:// --version";
    let result = checked(value, &all(value), &[], std::slice::from_ref(&(6..9)));
    for option in ["--help", "--version"] {
        assert_eq!(role(value, &result, option), Some(Role::Option));
    }
    let value = "--alphaxfile";
    let result = checked(
        value,
        &all(value),
        std::slice::from_ref(&(8..12)),
        std::slice::from_ref(&(7..8)),
    );
    assert_eq!(role(value, &result, "--alpha"), Some(Role::Option));
    assert_eq!(
        role(value, &result, "file"),
        None,
        "an argument chain cannot cross an equation boundary"
    );
}

#[test]
fn ellipsis_cannot_take_syntax_from_protected_atoms() {
    let value = "--alpha... --beta";
    let marked = [range(value, "--alpha"), range(value, "--beta")];
    for protected in [7..8, 8..9, 9..10, 7..10] {
        let result = checked(value, &marked, &[], std::slice::from_ref(&protected));
        assert_eq!(role(value, &result, "--beta"), Some(Role::Option));
        assert!(result.iter().all(|accent| accent.role == Role::Option));
        for byte in 7..10 {
            assert!(!result.iter().any(|accent| accent.bytes.contains(&byte)));
        }
    }
}

#[test]
fn protected_boundary_work_stays_linear_across_repeated_flags() {
    for count in [500, 1_000, 2_000] {
        let value = "--x... ".repeat(count);
        let marked: Vec<_> = (0..count).map(|index| index * 7..index * 7 + 3).collect();
        let protected: Vec<_> = (0..count)
            .map(|index| index * 7 + 4..index * 7 + 5)
            .collect();
        let mut work = Work::default();
        let result = accents_with_work(&value, &marked, &[], &protected, &mut work).unwrap();
        assert_eq!(result.len(), count);
        assert!(
            work.steps <= value.len() * 20,
            "{} steps for {} bytes",
            work.steps,
            value.len()
        );
    }
}
