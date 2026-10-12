//! Lexical contracts assert semantic roles; palette mapping is tested in theme.
use ratatui::style::{Modifier, Style};

use super::*;
use crate::theme;

fn highlight(value: &str) -> Vec<Accent> {
    CodeHighlights::new(value).accents
}

fn role_at(accents: &[Accent], source: &str, needle: &str) -> Option<StyleRole> {
    let position = source.find(needle).expect("fixture substring");
    accents
        .iter()
        .find(|accent| accent.bytes.contains(&position))
        .map(|accent| accent.role)
}

fn assert_text(accents: &[Accent], source: &str) {
    let mut previous = 0;
    for accent in accents {
        assert!(accent.bytes.start >= previous);
        assert!(accent.bytes.start < accent.bytes.end && accent.bytes.end <= source.len());
        assert!(source.is_char_boundary(accent.bytes.start));
        assert!(source.is_char_boundary(accent.bytes.end));
        previous = accent.bytes.end;
    }
    let mut code = CodeHighlights {
        accents: accents.to_vec(),
        ..Default::default()
    };
    let spans = code.spans(source, Style::new(), theme::DEFAULT_THEME.code_styles());
    assert_eq!(
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        source
    );
}

fn check(source: &str, expected: &[(&str, Option<StyleRole>)]) {
    let accents = highlight(source);
    for (needle, role) in expected {
        assert_eq!(
            role_at(&accents, source, needle),
            *role,
            "{source}: {needle}"
        );
    }
    assert_text(&accents, source);
}

#[test]
fn shell_arguments_variables_and_urls_have_independent_accents() {
    check(
        "curl https://example.org/if#fragment --output=\"$HOME/文件\" # download\n",
        &[
            ("curl", Some(StyleRole::CodeCommand)),
            ("https", None),
            ("/if#fragment", None),
            ("--output", Some(StyleRole::CodeOption)),
            ("$HOME", Some(StyleRole::CodeVariable)),
            ("/文件", Some(StyleRole::CodeString)),
            ("# download", Some(StyleRole::CodeComment)),
        ],
    );
}

#[test]
fn brackets_candidates_and_operators_have_distinct_semantic_roles() {
    check(
        "git [-v | --version] [-C <path>] [--output=<path|->] <command> [<args>]",
        &[
            ("git", Some(StyleRole::CodeCommand)),
            ("[", Some(StyleRole::CodeDelimiter)),
            ("]", Some(StyleRole::CodeDelimiter)),
            ("|", Some(StyleRole::CodeDelimiter)),
            ("<", Some(StyleRole::CodeDelimiter)),
            (">", Some(StyleRole::CodeDelimiter)),
            ("--version", Some(StyleRole::CodeOption)),
            ("path", Some(StyleRole::CodePlaceholder)),
            ("command", Some(StyleRole::CodePlaceholder)),
        ],
    );
    check(
        "tar {{[-c|--create]}} {{path/to/archive.tar}} {{路径/文件}}",
        &[
            ("{{", Some(StyleRole::CodeDelimiter)),
            ("[", Some(StyleRole::CodeDelimiter)),
            ("]", Some(StyleRole::CodeDelimiter)),
            ("|", Some(StyleRole::CodeDelimiter)),
            ("}}", Some(StyleRole::CodeDelimiter)),
            ("-c", Some(StyleRole::CodeOption)),
            ("--create", Some(StyleRole::CodeOption)),
            ("path/to", Some(StyleRole::CodePlaceholder)),
        ],
    );
    check(
        "git [--first <path>\n     | --second <other>]\ntemplate <typename T>",
        &[
            ("|", Some(StyleRole::CodeDelimiter)),
            ("<other>", Some(StyleRole::CodeDelimiter)),
            ("<typename", Some(StyleRole::CodeOperator)),
        ],
    );
    check(
        "cat < input >> output 2>&1 | grep x && echo ok || exit 1",
        &[
            ("<", Some(StyleRole::CodeOperator)),
            (">>", Some(StyleRole::CodeOperator)),
            (">&", Some(StyleRole::CodeOperator)),
            ("|", Some(StyleRole::CodeOperator)),
            ("&&", Some(StyleRole::CodeOperator)),
            ("||", Some(StyleRole::CodeOperator)),
        ],
    );
}

#[test]
fn dollar_markers_and_expansion_delimiters_are_independent_and_symmetric() {
    check(
        "echo ${10} $((count-1)) $(echo /* //path)",
        &[
            ("10", Some(StyleRole::CodeVariable)),
            ("count", Some(StyleRole::CodeVariable)),
            ("-1", Some(StyleRole::CodeOperator)),
            ("1))", Some(StyleRole::CodeNumber)),
            ("/*", Some(StyleRole::CodeOperator)),
            ("//path", None),
            (")", Some(StyleRole::CodeDelimiter)),
        ],
    );
    check(
        "  $ echo \"$HOME ${name} $? $$ $(command) $((1+2))\"\n$\n",
        &[
            ("$ echo", Some(StyleRole::CodePrompt)),
            ("$\n", Some(StyleRole::CodePrompt)),
            ("$HOME", Some(StyleRole::CodeVariable)),
            ("${", Some(StyleRole::CodeVariable)),
            ("{", Some(StyleRole::CodeDelimiter)),
            ("}", Some(StyleRole::CodeDelimiter)),
            ("name", Some(StyleRole::CodeVariable)),
            ("$?", Some(StyleRole::CodeVariable)),
            ("$$", Some(StyleRole::CodeVariable)),
            ("$(", Some(StyleRole::CodeVariable)),
            ("(", Some(StyleRole::CodeDelimiter)),
            (")", Some(StyleRole::CodeDelimiter)),
            ("command", Some(StyleRole::CodeCommand)),
            ("+", Some(StyleRole::CodeOperator)),
            ("1", Some(StyleRole::CodeNumber)),
        ],
    );
    check(
        r#"echo "$(printf '%s' "${outer:-${inner:-fallback}}")" suffix"#,
        &[
            ("printf", Some(StyleRole::CodeCommand)),
            ("%s", Some(StyleRole::CodeString)),
            ("outer", Some(StyleRole::CodeVariable)),
            ("inner", Some(StyleRole::CodeVariable)),
            (":-", Some(StyleRole::CodeOperator)),
            ("fallback", Some(StyleRole::CodeString)),
            ("}", Some(StyleRole::CodeDelimiter)),
            (")", Some(StyleRole::CodeDelimiter)),
            ("suffix", None),
        ],
    );
    check(
        "diff <(sort a) >(cat)\necho $((1+(2*3)))",
        &[
            ("<", Some(StyleRole::CodeOperator)),
            (">", Some(StyleRole::CodeOperator)),
            ("sort", Some(StyleRole::CodeCommand)),
            ("cat", Some(StyleRole::CodeCommand)),
            ("(", Some(StyleRole::CodeDelimiter)),
            (")", Some(StyleRole::CodeDelimiter)),
            ("*", Some(StyleRole::CodeOperator)),
        ],
    );
    check(
        "echo ${array[0]} ${name#prefix} ${path//old/new}",
        &[
            ("array", Some(StyleRole::CodeVariable)),
            ("[", Some(StyleRole::CodeDelimiter)),
            ("]", Some(StyleRole::CodeDelimiter)),
            ("0", Some(StyleRole::CodeNumber)),
            ("#", Some(StyleRole::CodeOperator)),
            ("prefix", Some(StyleRole::CodeString)),
            ("//", Some(StyleRole::CodeOperator)),
            ("old", Some(StyleRole::CodeString)),
        ],
    );
}

#[test]
fn compound_identifiers_and_paths_do_not_swallow_structural_punctuation() {
    check(
        "std::func(a[0]); obj.method(x)\npath/to/file[0-9] ./path(arg) /tmp/data{a,b}",
        &[
            ("std::func", None),
            ("obj.method", None),
            ("path/to", None),
            ("./path", None),
            ("/tmp/data", None),
            ("(", Some(StyleRole::CodeDelimiter)),
            (")", Some(StyleRole::CodeDelimiter)),
            ("[", Some(StyleRole::CodeDelimiter)),
            ("]", Some(StyleRole::CodeDelimiter)),
            ("{", Some(StyleRole::CodeDelimiter)),
            ("}", Some(StyleRole::CodeDelimiter)),
        ],
    );
    check(
        "curl https://[::1]/a(b)?x=1 --output file",
        &[
            ("https", None),
            ("[", None),
            ("]", None),
            ("(", None),
            (")", None),
            ("?", None),
            ("--output", Some(StyleRole::CodeOption)),
        ],
    );
}

#[test]
fn quoted_commented_and_escaped_brackets_are_not_structural_groups() {
    check(
        r#"echo '\$HOME (a)[b]{c}' "(x) [y] {z}" \(literal\) \[x\] # {ignored}"#,
        &[
            ("$HOME", Some(StyleRole::CodeString)),
            ("(a)", Some(StyleRole::CodeString)),
            ("(x)", Some(StyleRole::CodeString)),
            ("[y]", Some(StyleRole::CodeString)),
            ("{z}", Some(StyleRole::CodeString)),
            ("\\(", None),
            ("literal", None),
            ("\\[", None),
            ("# {", Some(StyleRole::CodeComment)),
        ],
    );
    check(
        "/* [ return 12;\n{} ] */\nreturn 34; // (ignored)\n'${path//old/new}'",
        &[
            ("[", Some(StyleRole::CodeComment)),
            ("return 12", Some(StyleRole::CodeComment)),
            ("return 34", Some(StyleRole::CodeKeyword)),
            ("(ignored)", Some(StyleRole::CodeComment)),
            ("${path", Some(StyleRole::CodeString)),
        ],
    );
    check(
        r#"echo "escaped \"quote\" \$HOME ${name:-value} $1 $?""#,
        &[
            ("\\$HOME", Some(StyleRole::CodeString)),
            ("$1", Some(StyleRole::CodeVariable)),
            ("name", Some(StyleRole::CodeVariable)),
            ("value", Some(StyleRole::CodeString)),
        ],
    );
}

#[test]
fn option_accents_stop_before_values_and_do_not_include_negative_numbers() {
    check(
        "gcc -D'NAME(x)=((x)+1)' -o output.o --count=12 --name=\"quoted\" -42 (-value)",
        &[
            ("-D", Some(StyleRole::CodeOption)),
            ("-o", Some(StyleRole::CodeOption)),
            ("--count", Some(StyleRole::CodeOption)),
            ("--name", Some(StyleRole::CodeOption)),
            ("NAME(x)", Some(StyleRole::CodeString)),
            ("output.o", None),
            ("12", Some(StyleRole::CodeNumber)),
            ("-42", None),
            ("-value", Some(StyleRole::CodeOperator)),
        ],
    );
}

#[test]
fn command_heads_require_prompt_builtin_or_unquoted_argument_evidence() {
    for (source, word) in [
        ("git status --short", "git"),
        ("git [-v|--version] <command>", "git"),
        ("mant <SELECTOR> [OPTIONS]", "mant"),
        ("git-show --stat", "git-show"),
        ("structural --no-return", "structural"),
        ("$ unknown-command", "unknown-command"),
        ("  $ echo hello", "echo"),
        ("printf \"%s\" \"$HOME\"", "printf"),
        ("tool {{path/to/file}}", "tool"),
    ] {
        check(source, &[(word, Some(StyleRole::CodeCommand))]);
    }
    for source in [
        "message \"--flag\"",
        "message # --flag",
        "message /* --flag */",
        "message // --flag",
        "message = --flag",
        "message (-value)",
        "message \"<value>\"",
        "message < value > limit",
        "ordinary identifier",
        "ordinary --no-return\nplain identifier",
    ] {
        let word = if source.starts_with("ordinary --") {
            "plain"
        } else {
            source.split_whitespace().next().unwrap()
        };
        check(source, &[(word, None)]);
    }
    for source in [
        "template <typename T>",
        "std::vector<T> values;",
        "if true; then echo ok; fi",
    ] {
        let accents = highlight(source);
        assert!(
            !accents
                .iter()
                .any(|accent| accent.role == StyleRole::CodeCommand),
            "{source}"
        );
        assert_text(&accents, source);
    }
}

#[test]
fn gcc_c_and_cpp_keywords_do_not_need_language_classification() {
    for source in [
        "struct S {\n  void foo () {};\n};",
        "struct S {\n};\n;",
        "struct S {\n  int a;\n  ;\n};",
        "struct A { int a; };\nstruct B : A { };",
        "void operator delete[] (void *) noexcept;",
        "void operator delete (void *, std::size_t) noexcept;",
    ] {
        let accents = highlight(source);
        for keyword in ["struct", "void", "int", "operator", "delete", "noexcept"] {
            if source.contains(keyword) {
                assert_eq!(
                    role_at(&accents, source, keyword),
                    Some(StyleRole::CodeKeyword)
                );
            }
        }
        for (index, c) in source.char_indices() {
            if "()[]{}".contains(c) {
                assert_eq!(
                    accents
                        .iter()
                        .find(|accent| accent.bytes.contains(&index))
                        .map(|accent| accent.role),
                    Some(StyleRole::CodeDelimiter)
                );
            }
        }
        assert_text(&accents, source);
    }
}

#[test]
fn shared_keywords_do_not_color_words_inside_paths_or_identifiers() {
    check(
        "if true; then return 12; fi\nfn main() { let value = 34; }\ndef run(): pass\ntool structural --no-return path/if example.com/for for-loop 用户if if用户",
        &[
            ("if true", Some(StyleRole::CodeKeyword)),
            ("then", Some(StyleRole::CodeKeyword)),
            ("return", Some(StyleRole::CodeKeyword)),
            ("fi\n", Some(StyleRole::CodeKeyword)),
            ("fn", Some(StyleRole::CodeKeyword)),
            ("let", Some(StyleRole::CodeKeyword)),
            ("def", Some(StyleRole::CodeKeyword)),
            ("pass", Some(StyleRole::CodeKeyword)),
            ("structural", None),
            ("path/if", None),
            ("example.com/for", None),
            ("for-loop", None),
            ("用户if", None),
            ("if用户", None),
        ],
    );
}

#[test]
fn preprocessor_headers_and_configuration_comments_use_local_rules() {
    check(
        "#include <stdio.h>\n# define NAME(x) (x)\n# shell comment\n; config comment\n;\nint value;\n",
        &[
            ("#include", Some(StyleRole::CodeKeyword)),
            ("<stdio.h>", Some(StyleRole::CodeString)),
            ("# define", Some(StyleRole::CodeKeyword)),
            ("# shell", Some(StyleRole::CodeComment)),
            ("; config", Some(StyleRole::CodeComment)),
            (";\nint", Some(StyleRole::CodeOperator)),
            ("int", Some(StyleRole::CodeKeyword)),
        ],
    );
}

#[test]
fn complete_block_projection_survives_every_unicode_inline_boundary() {
    let source = "echo \"$(printf '%s' \"${name:-e\u{301}多语言👩‍💻}\")\"\r\n";
    let base = Style::new().add_modifier(Modifier::UNDERLINED);
    let styles = theme::DEFAULT_THEME.code_styles();
    let expected = CodeHighlights::new(source).spans(source, base, styles);
    let expanded = |spans: Vec<Span<'_>>| {
        spans
            .into_iter()
            .flat_map(|span| {
                span.content
                    .chars()
                    .map(|c| (c, span.style))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let expected = expanded(expected);
    for boundary in source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([source.len()])
    {
        let mut code = CodeHighlights::new(source);
        let mut actual = code.spans(&source[..boundary], base, styles);
        actual.extend(code.spans(&source[boundary..], base, styles));
        assert!(
            actual
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        assert_eq!(expanded(actual), expected, "boundary {boundary}");
    }
}

#[test]
fn bounded_contexts_and_malformed_inputs_terminate_without_text_loss() {
    for source in [
        "{{".repeat(8192),
        "${".repeat(8192),
        ";".repeat(8192),
        "[".repeat(8192),
        "int ".repeat(4096),
        "${\"多语言".to_owned(),
        "{{\"多语言".to_owned(),
        "'\\".to_owned(),
        "/* open".to_owned(),
        "]})(".to_owned(),
    ] {
        assert_text(&highlight(&source), &source);
    }
    let source = format!(
        "{}value{}",
        "(".repeat(scan::MAX_CONTEXT_DEPTH + 1),
        ")".repeat(scan::MAX_CONTEXT_DEPTH + 1)
    );
    assert!(
        highlight(&source).is_empty(),
        "depth overflow must reject the entire block"
    );
    assert_text(&highlight(&source), &source);
    let source = "if true; then return 12; fi\n".repeat(1025);
    assert_eq!(
        role_at(&highlight(&source), &source, "if"),
        Some(StyleRole::CodeKeyword)
    );
    assert_text(&highlight(&source), &source);
}

#[test]
fn oversized_blocks_fall_back_as_a_whole_and_independent_blocks_reset() {
    let source = "return 12;\n".repeat(MAX_BLOCK_BYTES / 11 + 1);
    assert_eq!(highlight(&source), []);
    assert_text(&highlight(&source), &source);
    highlight("/* open");
    check("return 12;", &[("return", Some(StyleRole::CodeKeyword))]);
    assert_eq!(highlight(""), []);
}
