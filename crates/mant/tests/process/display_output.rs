#[cfg(feature = "roff")]
use super::process_support::run_text_command;
use super::process_support::{executable, run_text_input};
use std::fs;
use std::process::Command;

#[cfg(all(feature = "roff", target_os = "linux"))]
#[allow(unsafe_code)] // POSIX child setup: lower only this process's stack rlimit.
fn restrict_child_stack(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // Run the executable on the same 1 MiB main-stack budget that exposed the
    // debug Windows failure. No global limit changes or linker overrides.
    unsafe {
        command.pre_exec(|| {
            let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
            if libc::getrlimit(libc::RLIMIT_STACK, limit.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut limit = limit.assume_init();
            limit.rlim_cur = limit.rlim_cur.min(1024 * 1024);
            if libc::setrlimit(libc::RLIMIT_STACK, &raw const limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[test]
fn outline_reference_badges_survive_cli_color_policy() {
    let source = "# [Catalog](catalog.md#root)\n\n<!-- mant:entries role=command case=sensitive -->\n- [`run`](run.md#usage): [Body](body.md).\n";
    for color in ["never", "always"] {
        let output = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "markdown",
                "--outline",
                "--outline-entries",
                "all",
                "--outline-references",
                "all",
                "--format",
                "text",
                "--color",
                color,
            ],
            source,
        );
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("↗ catalog#root"), "{text}");
        assert!(text.contains("↗ run#usage"), "{text}");
        assert!(
            !text.contains("↗ body"),
            "body inventory is not a form capability"
        );
        assert_eq!(text.contains('\u{1b}'), color == "always");
    }
}

#[test]
#[cfg(feature = "roff")]
fn inline_roff_continuations_retain_indent_and_explicit_blank_lines() {
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "text",
            "--display",
            "direct",
        ],
        include_str!("../../../../tests/fixtures/roff/inline-definition-continuations.1"),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("-a  Initial.\n\n\n\n    INLINE_CONTINUATION."),
        "{text}"
    );
    for payload in ["CODE_CONTINUATION", "SECOND_CONTINUATION."] {
        assert!(
            text.lines().any(|line| line == format!("    {payload}")),
            "{text}"
        );
    }
    assert!(text.lines().any(|line| line == "--next-option"));
}

#[test]
fn source_rows_survive_plain_and_ansi_process_facades() {
    fn unstyle(text: &str) -> String {
        let mut parts = text.split('\u{1b}');
        let mut plain = parts.next().unwrap_or_default().to_owned();
        for part in parts {
            let (parameters, suffix) = part.strip_prefix('[').unwrap().split_once('m').unwrap();
            assert!(
                parameters
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == ';')
            );
            plain.push_str(suffix);
        }
        plain
    }
    for (format, source, expected) in [
        #[cfg(feature = "roff")]
        (
            "roff",
            ".TH PROBE 1\n.PD 2\n.SH FIRST\nALPHA\n.sp 3\n.SH SECOND\nBETA\n",
            "ALPHA\n\n\n\n\n\nSECOND\nBETA",
        ),
        #[cfg(feature = "roff")]
        (
            "roff",
            ".TH PROBE 1\n.SH TEST\n.nf\nALPHA\n.sp 1\n.sp 2\nBETA\n.fi\n",
            "ALPHA\n\n\n\nBETA",
        ),
        (
            "markdown",
            "# PROBE\n\n## TEST\n\nBEFORE\n\n```text\n\nALPHA\n\n\n```\n\nAFTER\n",
            "BEFORE\n\n\nALPHA\n\n\n\nAFTER",
        ),
    ] {
        for node in [false, true] {
            let mut args = vec![
                "--input",
                "-",
                "--input-format",
                format,
                "--format",
                "text",
                "--display",
                "direct",
                "--color",
                "never",
            ];
            if node {
                args.extend(["--node", "1"]);
            }
            let plain = run_text_input(&args, source);
            assert!(
                plain.status.success(),
                "{}",
                String::from_utf8_lossy(&plain.stderr)
            );
            let plain = String::from_utf8(plain.stdout).unwrap();
            // The first section excerpt deliberately excludes SECOND.
            if !(format == "roff" && source.contains(".SH SECOND") && node) {
                assert!(plain.contains(expected), "{source}\n{plain:?}");
            }
            let color = args.iter_mut().find(|value| **value == "never").unwrap();
            *color = "always";
            let colored = run_text_input(&args, source);
            assert!(
                colored.status.success(),
                "{}",
                String::from_utf8_lossy(&colored.stderr)
            );
            let colored = String::from_utf8(colored.stdout).unwrap();
            assert!(colored.contains('\u{1b}'));
            assert_eq!(unstyle(&colored), plain);
        }
    }
}

#[cfg(feature = "roff")]
#[test]
fn structural_heading_breaks_survive_the_cli_sanitization_boundary() {
    // The pinned CVS formatter recursively executes termp_an_pre(), placing
    // Alice on the next formatter row inside the Dq heading. Only the typed IR
    // LineBreak is trusted here; a newline embedded in source text remains
    // subject to terminal sanitization.
    let source = concat!(
        ".Dd September 13, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test\n",
        ".Sh Dq An -split An Alice\n.No BODY\n",
    );
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "text",
            "--color",
            "never",
        ],
        source,
    );
    assert!(output.status.success(), "{:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("“\nAlice”"), "{stdout:?}");
    assert!(!stdout.contains('�'), "{stdout:?}");

    let outline = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--outline",
            "--format",
            "text",
            "--color",
            "never",
        ],
        source,
    );
    assert!(outline.status.success(), "{:?}", outline.stderr);
    let outline = String::from_utf8(outline.stdout).unwrap();
    assert!(outline.contains("“ Alice”"), "{outline:?}");
    assert!(!outline.contains("“\nAlice”"), "{outline:?}");
    assert!(!outline.contains('�'), "{outline:?}");
}

#[cfg(feature = "roff")]
#[test]
fn incomplete_native_equation_warns_on_stderr_without_changing_document_text() {
    // The exact 260-deep equation was checked with the fixed -Ttree oracle;
    // CVS eqn.h keeps descendants beyond the owned transfer limit.
    let source = format!(
        ".TH DEEP 1\n.SH BODY\n.EQ\n{}x{}\n.EN\n",
        "sqrt { ".repeat(260),
        " }".repeat(260),
    );
    for format in ["text", "markdown", "json"] {
        let mut command = Command::new(executable());
        command.args([
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            format,
            "--color",
            "never",
        ]);
        #[cfg(target_os = "linux")]
        restrict_child_stack(&mut command);
        let output = run_text_command(command, &source);
        assert!(output.status.success(), "{:?}", output.stderr);
        let stderr = String::from_utf8(output.stderr).unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stderr.contains("source document content is incomplete"),
            "{stderr:?}"
        );
        assert!(!stdout.contains("source document content is incomplete"));
        if format == "json" {
            let _: mant_protocol::QueryBundle = serde_json::from_str(&stdout)
                .expect("the small-stack producer output must satisfy the actual wire decoder");
        }
    }
}

#[test]
fn clap_color_is_terminal_aware_and_explicitly_controllable() {
    let run = |arguments: &[&str]| {
        Command::new(executable())
            .args(arguments)
            .output()
            .expect("run mant color fixture")
    };

    let automatic = run(&["--help"]);
    assert!(automatic.status.success());
    assert!(!automatic.stdout.contains(&0x1b));

    let colored_help = run(&["--help", "--color", "always"]);
    assert!(colored_help.status.success());
    assert_eq!(colored_help.stderr.len(), 0);
    assert!(colored_help.stdout.contains(&0x1b));

    let plain_help = run(&["--help", "--color", "never"]);
    assert!(plain_help.status.success());
    assert!(!plain_help.stdout.contains(&0x1b));

    let colored_error = run(&["--color", "always"]);
    assert_eq!(colored_error.status.code(), Some(2));
    assert_eq!(colored_error.stdout.len(), 0);
    assert!(colored_error.stderr.contains(&0x1b));

    let colored_semantic_error = run(&[
        "git",
        "--display",
        "tui",
        "--format",
        "json",
        "--color",
        "always",
    ]);
    assert_eq!(colored_semantic_error.status.code(), Some(2));
    assert_eq!(colored_semantic_error.stdout.len(), 0);
    assert!(colored_semantic_error.stderr.contains(&0x1b));

    let missing_input = if cfg!(feature = "roff") {
        ["definitely-not-a-real-manual-for-colour", "--manual"]
    } else {
        ["--input", "definitely-not-a-real-markdown-for-colour.md"]
    };
    let colored_runtime_error = run(&[missing_input[0], missing_input[1], "--color", "always"]);
    assert_eq!(colored_runtime_error.status.code(), Some(1));
    assert_eq!(colored_runtime_error.stdout.len(), 0);
    assert!(colored_runtime_error.stderr.contains(&0x1b));

    let plain_runtime_error = run(&[missing_input[0], missing_input[1], "--color", "never"]);
    assert_eq!(plain_runtime_error.status.code(), Some(1));
    assert!(!plain_runtime_error.stderr.contains(&0x1b));

    let protocol = run(&["--protocol-version", "--compact", "--color", "always"]);
    assert!(protocol.status.success());
    assert!(!protocol.stdout.contains(&0x1b));
    serde_json::from_slice::<serde_json::Value>(&protocol.stdout).expect("plain protocol JSON");
}

#[test]
fn self_manual_help_respects_disabled_colour_and_dumb_terminals() {
    for (key, value) in [("NO_COLOR", "1"), ("TERM", "dumb")] {
        let output = Command::new(executable())
            .arg("--help")
            .env_remove("CLICOLOR_FORCE")
            .env(key, value)
            .output()
            .expect("uncoloured help");
        assert!(output.status.success());
        assert_eq!(output.stderr.len(), 0);
        assert!(!output.stdout.contains(&0x1b));
        assert!(String::from_utf8_lossy(&output.stdout).contains("ManT manual:"));
    }
}

#[test]
fn partial_query_text_is_colored_only_when_the_stream_policy_allows_it() {
    let path = std::env::temp_dir().join(format!(
        "mant-colored-query-process-{}.md",
        std::process::id()
    ));
    fs::write(
        &path,
        "# demo\n\n## Options\n\n<!-- mant:entries role=option -->\n- `--color WHEN`: Select terminal color.\n",
    )
    .expect("write colored query fixture");
    let run = |arguments: &[&str]| {
        Command::new(executable())
            .arg("--input")
            .arg(&path)
            .args(arguments)
            .output()
            .expect("run colored query fixture")
    };

    let automatic = run(&["--explain=--color"]);
    assert!(automatic.status.success(), "{automatic:?}");
    assert!(!automatic.stdout.contains(&0x1b));

    let colored = run(&["--explain=--color", "--color", "always"]);
    assert!(colored.status.success(), "{colored:?}");
    assert!(colored.stdout.contains(&0x1b));

    let plain = run(&["--search", "color", "--color", "never"]);
    assert!(plain.status.success(), "{plain:?}");
    assert!(!plain.stdout.contains(&0x1b));

    let json = run(&[
        "--search",
        "color",
        "--format",
        "json",
        "--compact",
        "--color",
        "always",
    ]);
    assert!(json.status.success(), "{json:?}");
    assert!(!json.stdout.contains(&0x1b));
    serde_json::from_slice::<serde_json::Value>(&json.stdout).expect("plain search JSON");

    fs::remove_file(path).expect("remove colored query fixture");
}
