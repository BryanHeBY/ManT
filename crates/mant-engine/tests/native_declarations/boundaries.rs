//! All producer paths retain literal/argument boundaries before name binding.
use super::*;

fn names(source: &str) -> Vec<String> {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let item = definitions(content.document.as_ref().unwrap())[0];
    item.entry.as_ref().unwrap().names.clone()
}

#[test]
fn plain_arguments_do_not_restart_command_names() {
    for argument in ["option|other", "target, destination"] {
        let source =
            format!(".TH PROBE 1\n.SH COMMANDS\n.TP\n\\fBstart\\fR {argument}\nDocumented body.\n");
        assert_eq!(names(&source), ["start"], "{source}");
    }
    assert_eq!(
        names(".TH PROBE 1\n.SH COMMANDS\n.TP\n\\fBstart\\fR target, \\fBstop\\fR\nBody.\n"),
        ["start", "stop"]
    );
}

#[test]
fn native_environment_templates_are_not_exact_prefix_names() {
    for (head, expected) in [
        ("Ev GIT_CONFIG_KEY_ Ns Ar n", None),
        ("Ev DEMO_ Ns Ar NAME", None),
        ("Ev DEMO_HOME Ar directory", Some("DEMO_HOME")),
        ("Ev DEMO_HOME Ns = Ns Ar value", Some("DEMO_HOME")),
        ("Ev DEMO_ Ns No SUFFIX", Some("DEMO_SUFFIX")),
    ] {
        let source = format!(
            ".Dd September 8, 2026\n.Dt PROBE 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {head}\nDocumented body.\n.El\n"
        );
        let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let item = definitions(content.document.as_ref().unwrap())[0];
        let entry = item.entry.as_ref().unwrap();
        assert_eq!(entry.kind, mant_ir::EntryKind::EnvironmentVariable);
        assert_eq!(
            entry.names,
            expected.into_iter().collect::<Vec<_>>(),
            "{head}"
        );
        assert!(!item.terms.is_empty() && !item.description.is_empty());
    }
}

#[test]
fn pattern_heads_preserve_independently_spelled_long_options() {
    let source = ".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -# --rapid --dense\nDocumented body.\n";
    assert_eq!(names(source), ["--rapid", "--dense"]);
    for head in [
        ".BI \"-# \" \"--rapid --dense\"",
        ".B -# FILE --rapid",
        ".B -# --rapid=value --dense",
    ] {
        let source = format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n{head}\nBody.\n");
        assert!(!names(&source).contains(&"--dense".to_owned()), "{head}");
    }
}
