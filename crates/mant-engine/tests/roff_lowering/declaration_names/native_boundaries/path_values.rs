//! Path assignments preserve authored punctuation and proven operand joins.

use super::check;

#[test]
fn assigned_paths_and_dotted_values_stop_before_an_independent_aside() {
    // mdoc_macro.c::in_line gives Pa its own operand; mdoc_pa_pre
    // preserves that path role in HTML. The pinned devices agree on the
    // actual visible right-hand sides, including source punctuation.
    for (head, names, label, full) in [
        ("Fl a= Ns Pa file", &["-a"][..], "-a=file", true),
        (
            "Fl a= Ns Pa /tmp/test.conf",
            &["-a"],
            "-a=/tmp/test.conf",
            true,
        ),
        (
            r"Fl a Ns Li \&= Ns Pa ./file.tar.gz",
            &["-a"],
            "-a=./file.tar.gz",
            true,
        ),
        (
            "Fl m Ar file , Fl Fl master-key= Ns Pa file",
            &["-m", "--master-key"],
            "-m file, --master-key=file",
            true,
        ),
        (
            "Fl d Ar file , Fl Fl database= Ns Pa file",
            &["-d", "--database"],
            "-d file, --database=file",
            true,
        ),
        ("Fl a Ns = Ns Li 127.0.0.1", &["-a"], "-a=127.0.0.1", true),
        (
            "Fl a Ns = Ns Li example.org",
            &["-a"],
            "-a=example.org",
            true,
        ),
        ("Fl a Ns = Ns Li 1.25", &["-a"], "-a=1.25", true),
        (
            "Fl a Ns = Ns Li 1.25 Ns Op Ar param",
            &["-a"],
            "-a=1.25[param]",
            true,
        ),
        (
            "Fl a Ns = Ns Ar x Ns Po Ar port Pc",
            &["-a"],
            "-a=x(port)",
            true,
        ),
        (
            "Fl a Ns = Ns Ar x Op Ar extra",
            &["-a"],
            "-a=x [extra]",
            true,
        ),
        (
            r#"Fl a= Ns Ar x No "(default: none)""#,
            &["-a"],
            "-a=x (default: none)",
            false,
        ),
        (
            r#"Fl a Ns Li \&= Ns Ar x No "(deprecated)""#,
            &["-a"],
            "-a=x (deprecated)",
            false,
        ),
        (
            "Fl a Ns = Ns Ar x Po default: none Pc",
            &["-a"],
            "-a=x (default: none)",
            false,
        ),
        (
            r#"Fl a Ns = Ns Li 1.25 No "(arbitrary: hint)""#,
            &["-a"],
            "-a=1.25 (arbitrary: hint)",
            false,
        ),
        ("Fl a= Ns Ar x Pa file", &["-a"], "-a=x file", false),
        (
            "Fl f , -vdev-file-directory Ns = (default: Pa /tmp )",
            &["-f", "--vdev-file-directory"],
            "-f, --vdev-file-directory= (default: /tmp)",
            false,
        ),
    ] {
        check(
            "OPTIONS",
            head,
            names,
            label,
            full,
            &["file", "x", "127.0.0.1", "param", "default"],
        );
    }
}

#[test]
fn an_executed_path_can_consist_only_of_punctuation() {
    // CVS mdoc_pa_pre() marks these complete words as paths. The same
    // visible punctuation under Li has no pathname evidence by itself.
    for role in ["Pa", "Li"] {
        for (operand, visible) in [
            ("/", "/"),
            (r"\&.", "."),
            (r"\&..", ".."),
            ("./", "./"),
            ("../", "../"),
        ] {
            let head = format!("Fl a= Ns {role} {operand}");
            check(
                "OPTIONS",
                &head,
                &["-a"],
                &format!("-a={visible}"),
                role == "Pa",
                &[visible],
            );
        }
    }
}

#[test]
fn attached_path_pieces_stop_at_real_word_spacing() {
    // The fixed tree and HTML have two Pa operands even when Ns makes
    // their visible bytes touch. termp_ns_pre() controls that boundary.
    for prefix in ["/", "../", "/tmp/"] {
        for (after, gap) in [("Ns Pa foo", ""), ("Ns Li foo", ""), ("Pa foo", " ")] {
            let head = format!("Fl a= Ns Pa {prefix} {after}");
            let label = format!("-a={prefix}{gap}foo");
            check("OPTIONS", &head, &["-a"], &label, gap.is_empty(), &["foo"]);
        }
    }
}

#[test]
fn pathname_separators_can_begin_the_touching_operand() {
    // Fixed mdoc Ns suppresses the word separator before the right-hand
    // slash. A visible space is a real boundary in the other forms.
    for (path, full) in [
        ("Pa /var Ns Pa /log", true),
        ("Pa /var Ns Li /log", true),
        ("Pa /var Ns No /log", true),
        ("Pa /var Li /log", false),
        ("Pa /var Li / Ns Pa log", false),
    ] {
        let head = format!("Fl a= Ns {path}");
        let label = if full { "-a=/var/log" } else { "-a=/var /log" };
        check("OPTIONS", &head, &["-a"], label, full, &["/log"]);
    }
}

#[test]
fn dotted_values_require_a_native_join_and_a_typed_terminal_operand() {
    // termp_quote_pre/post and termp_fl_pre preserve these spellings. A
    // detached dot word is not authored as either half of the assignment.
    for (head, label, complete) in [
        (
            r"Fl a= Ns Ar x Pq Ar value | No \&. Ns Pa config",
            "-a=x (value | .config)",
            true,
        ),
        (
            "Fl a= Ns Ar x Pq Ar value | No .bak",
            "-a=x (value | .bak)",
            false,
        ),
        (
            r"Fl a= Ns Ar x Pq Li auto | Li \&. Ns Pa config",
            "-a=x (auto | .config)",
            true,
        ),
        (
            "Fl a= Ns Ar x Pq Li auto | Pa .config",
            "-a=x (auto | .config)",
            true,
        ),
        (
            r"Fl a= Ns Ar x Pq Li auto Li \&. Ns Pa config",
            "-a=x (auto .config)",
            false,
        ),
        ("Fl a= Ns Pa file No .bak .old", "-a=file .bak .old", false),
        ("Fl a= Ns Pa file No .bak", "-a=file .bak", false),
        ("Fl a= Ns Pa file Pa .bak", "-a=file .bak", false),
        ("Fl a= Ns Pa file Ns No .bak", "-a=file.bak", true),
        (r"Fl a= Ns Ar x Op Ar key\&:", "-a=x [key:]", true),
        (
            r"Fl a= Ns Ar x Op Ar key\&: Ns Li auto",
            "-a=x [key:auto]",
            true,
        ),
    ] {
        check(
            "OPTIONS",
            head,
            &["-a"],
            label,
            complete,
            &["config", "auto", ".bak", "-b"],
        );
    }
}
