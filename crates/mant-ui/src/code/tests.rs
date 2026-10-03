use ratatui::style::{Color, Modifier};

use super::*;

fn highlight(value: &str) -> Vec<Span<'static>> {
    CodeHighlights::new(value).spans(value, theme::style(StyleRole::Text))
}

fn color_at(spans: &[Span<'_>], source: &str, needle: &str) -> Option<Color> {
    let position = source.find(needle).expect("fixture substring");
    let mut offset = 0;
    for span in spans {
        offset += span.content.len();
        if position < offset {
            return span.style.fg;
        }
    }
    panic!("missing accented byte");
}

fn assert_text(spans: &[Span<'_>], source: &str) {
    assert_eq!(
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>(),
        source
    );
}

#[test]
fn shell_arguments_variables_and_urls_have_independent_accents() {
    let source = "curl https://example.org/if#fragment --output=\"$HOME/文件\" # download\n";
    let spans = highlight(source);
    for (needle, color) in [
        ("curl", theme::PEACH),
        ("https", theme::TEXT),
        ("/if#fragment", theme::TEXT),
        ("--output", theme::GREEN),
        ("$HOME", theme::PINK),
        ("/文件", theme::BLUE),
        ("# download", theme::SUBTEXT),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
}

#[test]
fn shell_prompt_and_expansion_dollars_have_distinct_roles() {
    let source = "  $ echo \"$HOME ${name} $? $$ $(command) $((1+2))\"\n$\n";
    let spans = highlight(source);
    for (needle, color) in [
        ("$ echo", theme::SUBTEXT),
        ("$HOME", theme::PINK),
        ("${name}", theme::PINK),
        ("$?", theme::PINK),
        ("$$", theme::PINK),
        ("$(command)", theme::PINK),
        ("$((1+2))", theme::PINK),
        ("$\n", theme::SUBTEXT),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
    let source = "echo \\$HOME '$HOME' \"\\$HOME\"";
    let spans = highlight(source);
    assert_eq!(color_at(&spans, source, "\\$HOME"), Some(theme::TEXT));
    assert_eq!(color_at(&spans, source, "'$HOME'"), Some(theme::BLUE));
    assert_text(&spans, source);
}

#[test]
fn option_accents_stop_before_values_and_do_not_include_negative_numbers() {
    let source = "gcc -D'NAME(x)=((x)+1)' -o output.o --count=12 --name=\"quoted\" -42 (-value) value - other";
    let spans = highlight(source);
    for needle in ["-D", "-o", "--count", "--name"] {
        assert_eq!(color_at(&spans, source, needle), Some(theme::GREEN));
    }
    for (needle, color) in [
        ("NAME(x)", theme::BLUE),
        ("output.o", theme::TEXT),
        ("12", theme::YELLOW),
        ("-42", theme::TEXT),
        ("-value", theme::TEXT),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
}

#[test]
fn git_synopsis_configuration_and_tldr_placeholders_keep_original_syntax() {
    for source in [
        "git [-v | --version] [-C <path>] [--git-dir=<path>] <command> [<args>]",
        "[core]\n  filemode = false\n; user identity\n[user]\n  name = \"Example\"\n",
        "tar {{[-c|--create]}} {{path/to/archive.tar}} {{路径/文件}}",
        "tool --name=\"{{值}}\"",
    ] {
        let spans = highlight(source);
        assert_text(&spans, source);
        assert!(
            spans
                .iter()
                .any(|span| span.style.fg == Some(theme::SUBTEXT_BRIGHT))
        );
    }
    let source = "git [-v | --version] <command> {{[-c|--create]}}";
    let spans = highlight(source);
    for (needle, color) in [
        ("git", theme::PEACH),
        ("[", theme::SUBTEXT_BRIGHT),
        ("|", theme::SUBTEXT_BRIGHT),
        ("<", theme::SUBTEXT_BRIGHT),
        (">", theme::SUBTEXT_BRIGHT),
        ("--version", theme::GREEN),
        ("command", theme::TEXT),
        ("-c|--create", theme::TEXT),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert!(spans.iter().any(|span| span.content == "[-c|--create]"));
    let source = "git [--output=<path|->] [<args>]";
    let spans = highlight(source);
    for marker in ["[", "]", "<", ">", "|"] {
        assert_eq!(
            color_at(&spans, source, marker),
            Some(theme::SUBTEXT_BRIGHT)
        );
    }
    assert_eq!(color_at(&spans, source, "path"), Some(theme::TEXT));
    assert_eq!(color_at(&spans, source, "--output"), Some(theme::GREEN));
    assert_text(&spans, source);
    let source = "git [--first <path>\n     | --second <other>]\ntemplate <typename T>";
    let spans = highlight(source);
    assert_eq!(color_at(&spans, source, "|"), Some(theme::SUBTEXT_BRIGHT));
    assert_eq!(
        color_at(&spans, source, "<other>"),
        Some(theme::SUBTEXT_BRIGHT)
    );
    assert_eq!(color_at(&spans, source, "<typename"), Some(theme::TEXT));
    assert_text(&spans, source);
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
        let spans = highlight(source);
        assert_eq!(
            color_at(&spans, source, word),
            Some(theme::PEACH),
            "{source}"
        );
        assert_text(&spans, source);
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
        let spans = highlight(source);
        let word = if source.starts_with("ordinary --") {
            "plain"
        } else {
            source.split_whitespace().next().unwrap()
        };
        assert_eq!(
            color_at(&spans, source, word),
            Some(theme::TEXT),
            "{source}"
        );
        assert_text(&spans, source);
    }
    for source in [
        "template <typename T>",
        "std::vector<T> values;",
        "if true; then echo ok; fi",
    ] {
        let spans = highlight(source);
        assert!(
            !spans.iter().any(|span| span.style.fg == Some(theme::PEACH)),
            "{source}"
        );
        if source.contains('<') {
            assert_eq!(color_at(&spans, source, "<"), Some(theme::TEXT));
        }
        assert_text(&spans, source);
    }
}

#[test]
fn gcc_c_and_cpp_keywords_do_not_need_language_classification() {
    // Independently selected lexical forms from gcc's -Wpedantic,
    // -Wno-inaccessible-base and -Wsized-deallocation examples.
    for source in [
        "struct S {\n  void foo () {};\n};",
        "struct S {\n};\n;",
        "struct S {\n  int a;\n  ;\n};",
        "struct A { int a; };\nstruct B : A { };\nstruct C : B, A { };",
        "void operator delete (void *) noexcept;\nvoid operator delete[] (void *) noexcept;",
        "void operator delete (void *, std::size_t) noexcept;",
    ] {
        let spans = highlight(source);
        for keyword in ["struct", "void", "int", "operator", "delete", "noexcept"] {
            if source.contains(keyword) {
                assert_eq!(
                    color_at(&spans, source, keyword),
                    Some(theme::MAUVE),
                    "{source}: {keyword}"
                );
            }
        }
        assert_text(&spans, source);
    }
}

#[test]
fn shared_keywords_do_not_color_words_inside_paths_or_identifiers() {
    let source = "if true; then return 12; fi\nfn main() { let value = 34; }\ndef run(): pass\ntool structural --no-return path/if example.com/for for-loop 用户if if用户";
    let spans = highlight(source);
    for keyword in [
        "if true", "then", "return", "fi\n", "fn", "let", "def", "pass",
    ] {
        assert_eq!(
            color_at(&spans, source, keyword),
            Some(theme::MAUVE),
            "{keyword}"
        );
    }
    for word in [
        "structural",
        "path/if",
        "example.com/for",
        "for-loop",
        "用户if",
        "if用户",
    ] {
        assert_eq!(color_at(&spans, source, word), Some(theme::TEXT), "{word}");
    }
    assert_text(&spans, source);
}

#[test]
fn multiline_quotes_and_comments_do_not_color_inner_keywords() {
    let source =
        "/* note\nreturn 12;\n*/\nreturn 34; // note\n\"literal\nstruct $HOME\"\n'if $USER'\n";
    let spans = highlight(source);
    for (needle, color) in [
        ("return 12", theme::SUBTEXT),
        ("return 34", theme::MAUVE),
        ("// note", theme::SUBTEXT),
        ("struct", theme::BLUE),
        ("$HOME", theme::PINK),
        ("$USER", theme::BLUE),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
}

#[test]
fn escaped_quotes_markers_and_variable_expansions_remain_literal() {
    let source =
        r#"echo "escaped \"quote\" \$HOME ${name:-value} $1 $?" \{\{值\}\} '${path//old/new}'"#;
    let spans = highlight(source);
    for (needle, color) in [
        ("\\$HOME", theme::BLUE),
        ("${name:-value}", theme::PINK),
        ("$1", theme::PINK),
        ("${path//old/new}", theme::BLUE),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
}

#[test]
fn preprocessor_headers_and_configuration_comments_use_local_rules() {
    let source = "#include <stdio.h>\n# define NAME(x) (x)\n# shell comment\n; config comment\n;\nint value;\n";
    let spans = highlight(source);
    for (needle, color) in [
        ("#include", theme::MAUVE),
        ("<stdio.h>", theme::BLUE),
        ("# define", theme::MAUVE),
        ("# shell", theme::SUBTEXT),
        ("; config", theme::SUBTEXT),
        (";\nint", theme::TEXT),
        ("int", theme::MAUVE),
    ] {
        assert_eq!(color_at(&spans, source, needle), Some(color), "{needle}");
    }
    assert_text(&spans, source);
}

#[test]
fn complete_block_scanning_survives_every_unicode_inline_boundary() {
    let source = "/* 多语言\ncomment */\nconst char *s = \"e\u{301} 👩‍💻 $HOME\";\r\n";
    let expected = highlight(source);
    for boundary in source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([source.len()])
    {
        let mut code = CodeHighlights::new(source);
        let base = Style::new().add_modifier(Modifier::UNDERLINED);
        let mut actual = code.spans(&source[..boundary], base);
        actual.extend(code.spans(&source[boundary..], base));
        assert_text(&actual, source);
        assert!(
            actual
                .iter()
                .all(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        for needle in ["多语言", "comment", "const", "e\u{301}", "👩‍💻", "$HOME"] {
            assert_eq!(
                color_at(&actual, source, needle),
                color_at(&expected, source, needle),
                "boundary {boundary}"
            );
        }
    }
}

#[test]
fn malformed_markers_and_long_rows_terminate_without_text_loss() {
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
    ] {
        assert_text(&highlight(&source), &source);
    }
    let source = "if true; then return 12; fi\n".repeat(1025);
    let spans = highlight(&source);
    assert_eq!(color_at(&spans, &source, "if"), Some(theme::MAUVE));
    assert_text(&spans, &source);
}

#[test]
fn oversized_blocks_fall_back_as_a_whole_and_independent_blocks_reset() {
    let source = "return 12;\n".repeat(MAX_BLOCK_BYTES / 11 + 1);
    assert_eq!(
        highlight(&source),
        vec![Span::styled(source, theme::style(StyleRole::Text))]
    );
    highlight("/* open");
    let source = "return 12;";
    assert_eq!(
        color_at(&highlight(source), source, "return"),
        Some(theme::MAUVE)
    );
    assert_eq!(highlight(""), Vec::<Span<'_>>::new());
}
