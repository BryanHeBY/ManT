//! Exact names and queryable signatures follow the native operand boundaries.

use super::{ExplanationOptions, ExplanationQuery, assert_name_positions, owners};
use crate::json_roundtrip::json_string_roundtrip;
use mant_ir::inline_plain_text;

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";

fn check(section: &str, head: &str, names: &[&str], label: &str, full: bool, hidden: &[&str]) {
    let source =
        format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nOwner proof.\n.El\n");
    let native = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&native);
    let entries = owners(content.document.as_ref().unwrap());
    let [owner] = entries.as_slice() else {
        panic!("expected one native It owner: {source}");
    };
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, names, "{head}");
    let actual = owner
        .forms()
        .unwrap()
        .iter()
        .map(inline_plain_text)
        .collect::<Vec<_>>();
    let forms = if full {
        std::slice::from_ref(&label)
    } else {
        names
    };
    assert_eq!(actual, forms, "{head}");
    for name in names {
        assert_name_positions(&content, *owner, name);
    }
    for (query, expected) in names
        .iter()
        .map(|name| (*name, true))
        .chain(std::iter::once((label, full)))
        .chain(hidden.iter().map(|name| (*name, false)))
    {
        let explanation = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: query.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            explanation.counts.direct_entry.total,
            u32::from(expected),
            "{head}: {query}"
        );
        if expected {
            assert!(mant_render::render_explanation_text(&explanation).contains("Owner proof."));
        }
    }
}

#[test]
fn authored_brackets_cross_native_children_without_absorbing_following_aliases() {
    // blk_part_exp emits BLOCK-leading punctuation before its HEAD and
    // BODY. All examples were compared with CVS tree, HTML and both
    // terminal devices. Pipe operands are distinct from comma aliases.
    for (head, names, form) in [
        (
            "Nm alpha Po ( Po Ar file ) Pc Pc | Nm beta , Nm gamma",
            &["gamma"][..],
            "alpha (((file))) | beta, gamma",
        ),
        (
            "Nm alpha Po Po ( Ar x Pc Pc ) , Nm gamma",
            &["alpha", "gamma"],
            "alpha (((x))), gamma",
        ),
        (
            "Nm alpha ( Po Po Ar x Pc Pc ) , Nm gamma",
            &["alpha", "gamma"],
            "alpha (((x))), gamma",
        ),
        (
            "Nm alpha ( Po Ar file ) Pc , Nm gamma",
            &["alpha", "gamma"],
            "alpha ((file)), gamma",
        ),
        (
            "Nm alpha Oo Oo [ Ar x Oc Oc ] , Nm gamma",
            &["alpha", "gamma"],
            "alpha [[[x]]], gamma",
        ),
    ] {
        check(
            "DESCRIPTION",
            head,
            names,
            form,
            true,
            &["beta", "file", "x"],
        );
    }
}

#[test]
fn native_assignment_forms_require_a_complete_right_hand_side() {
    // Fl, Li, Ar and the enclosure handlers can execute separate words.
    // The fixed profiles confirm each visible assignment and its It BODY.
    for equals in ["=", r"Li \&=", r#"Li "=""#, r"No \&="] {
        for (rhs, form, full) in [
            ("Ar x", "-a=x", true),
            ("Op Ar x", "-a=[x]", true),
            ("Oo Ar x Oc", "-a=[x]", true),
            ("Po Ar x Pc", "-a=(x)", true),
            ("Bro Ar x Brc", "-a={x}", true),
            ("Ar x Ns Li / Ns Ar y", "-a=x/y", true),
            ("Ar x No this is prose", "-a=x this is prose", false),
            ("Ar x Ns Ev FAKE", "-a=xFAKE", false),
            ("Ar x Ns Li / Ns Ev FAKE", "-a=x/FAKE", false),
            ("Op Ar x Li tail", "-a=[x tail]", false),
        ] {
            let head = format!("Fl a Ns {equals} Ns {rhs}");
            check("OPTIONS", &head, &["-a"], form, full, &["x", "y", "FAKE"]);
        }
    }
    check(
        "DESCRIPTION",
        r"Fl a Ns Li \&= Ns Ar x Lk https://example.invalid docs",
        &["-a"],
        "-a=x docs: https://example.invalid",
        false,
        &["x", "docs"],
    );
}

#[test]
fn the_flag_word_proves_its_own_value_without_proving_independent_operands() {
    // CVS tree shows the value inside the Fl word and the Li/Ev suffixes
    // outside it. term.c::encode1() overprints A with B in the clipped form.
    for (head, label, full) in [
        (r"Fl q=\zAB", "-q=B", true),
        ("Fl q=FILE Ns Li bogus", "-q=FILEbogus", false),
        ("Fl q=FILE Ns Ev bogus", "-q=FILEbogus", false),
        (r"Fl q=\zAB Ns Li bogus", "-q=Bbogus", false),
    ] {
        check("OPTIONS", head, &["-q"], label, full, &["bogus"]);
    }
}

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
fn assignment_groups_prove_their_content_and_dots_require_adjacency() {
    // mdoc_macro.c::blk_part_imp/blk_part_exp and termp_ns_pre() preserve
    // the delimiters and operand adjacency in the fixed CVS output.
    for (head, label, full) in [
        ("Fl a= Ns Ar x [ Ar opt ]", "-a=x [opt]", true),
        ("Fl a= Ns Ar x Op Ar opt", "-a=x [opt]", true),
        ("Fl a= Ns Ar x ( Ar port )", "-a=x (port)", true),
        ("Fl a= Ns Ar x Po Ar port Pc", "-a=x (port)", true),
        (
            r#"Fl a= Ns Ar x Po "default:" Pa /tmp Pc"#,
            "-a=x (default: /tmp)",
            false,
        ),
        (
            r#"Fl a= Ns Ar x Po "default:" Ar none Pc"#,
            "-a=x (default: none)",
            false,
        ),
        ("Fl a= Ns Li example.com", "-a=example.com", true),
        ("Fl a= Ns Li example Ns Li .com", "-a=example.com", true),
        (
            r"Fl a= Ns Li example Ns Li \&. Ns Li com",
            "-a=example.com",
            true,
        ),
        (
            r"Fl a= Ns Li example Ns Li \&. No Deprecated",
            "-a=example. Deprecated",
            false,
        ),
    ] {
        check(
            "OPTIONS",
            head,
            &["-a"],
            label,
            full,
            &["port", "opt", "none", "/tmp", "Deprecated"],
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
fn detached_groups_admit_literal_alternatives_with_actual_argument_evidence() {
    // The fixed tree distinguishes Li, No and Em even when term_word()
    // makes their text identical. Both terminal devices and HTML agree
    // on the complete HEAD and leave BODY outside its delimiters.
    for (open, close, left, right) in [
        ("Po", " Pc", '(', ')'),
        ("Pq", "", '(', ')'),
        ("Bq", "", '[', ']'),
        ("Oo", " Oc", '[', ']'),
        ("[", " ]", '[', ']'),
    ] {
        for (body, visible, full) in [
            ("Ar opt | Li auto", "opt | auto", true),
            ("Li auto | Ar opt", "auto | opt", true),
            ("Ar opt Ns = Ns Li auto", "opt=auto", true),
            ("Ar opt , Li auto", "opt, auto", true),
            ("Ar opt Ns / Ns Li auto", "opt/auto", true),
            ("Ar port Ns Li .local", "port.local", true),
            ("Ar opt | No deprecated", "opt | deprecated", false),
            ("Ar opt , No deprecated", "opt, deprecated", false),
            ("Ar opt Ns / Ns No deprecated", "opt/deprecated", false),
            ("Ar port Li .local", "port .local", false),
            ("Ar port Ns No .local", "port.local", false),
            ("Ar opt | Em auto", "opt | auto", false),
            ("No default : Pa /tmp", "default: /tmp", false),
            ("Ar opt Li tail", "opt tail", false),
        ] {
            let head = format!("Fl a= Ns Ar x {open} {body}{close}");
            let label = format!("-a=x {left}{visible}{right}");
            check(
                "OPTIONS",
                &head,
                &["-a"],
                &label,
                full,
                &["opt", "auto", "/tmp"],
            );
        }
    }
    for (head, label) in [
        (
            r#"Fl a= Ns Ar x Bq "default:" Pa /tmp"#,
            "-a=x [default: /tmp]",
        ),
        ("Fl a= Ns Ar x [ default: none ]", "-a=x [default: none]"),
    ] {
        check("OPTIONS", head, &["-a"], label, false, &["none", "/tmp"]);
    }
}

#[test]
fn native_optional_arguments_keep_real_ntp_keygen_signatures() {
    // FreeBSD ntp-keygen(8), reduced to two It declarations. Fixed CVS
    // mdoc_macro.c executes Op/Oo bodies and their generated delimiters.
    // An empty Ar before Oo supplies "file ..." from validation.
    for (head, names, form) in [
        (
            r"Fl s Fl \-subject\-key Ns = Ar Oo host Oc Op @ Ar group",
            &["-s", "--subject-key"][..],
            "-s --subject-key= file ... [host] [@ group]",
        ),
        (
            r"Fl S Fl \-sign\-key Ns = Op Cm RSA | DSA",
            &["-S", "--sign-key"],
            "-S --sign-key= [RSA | DSA]",
        ),
    ] {
        check(
            "OPTIONS",
            head,
            names,
            form,
            true,
            &["host", "group", "RSA", "DSA"],
        );
    }
}

#[test]
fn native_optionality_proves_a_value_without_admitting_explanatory_text() {
    // Fixed CVS termp_quote_pre()/post() print [] for both Bq and Oo.
    // The tree distinguishes the optional construct and the quoting macro.
    for (head, label, full) in [
        ("Fl a= Ns Ar x Bq host", "-a=x [host]", false),
        ("Fl a= Ns Ar x Oo host Oc", "-a=x [host]", true),
        (
            "Fl a= Ns Ar x Oo No deprecated Oc",
            "-a=x [deprecated]",
            false,
        ),
        ("Fl a= Ns Ar x Op No deprecated", "-a=x [deprecated]", false),
        (
            r#"Fl a= Ns Ar x Oo "default:" Pa /tmp Oc"#,
            "-a=x [default: /tmp]",
            false,
        ),
        (
            "Fl a= Ns Ar x Oo Ar opt | No deprecated Oc",
            "-a=x [opt | deprecated]",
            false,
        ),
    ] {
        check(
            "OPTIONS",
            head,
            &["-a"],
            label,
            full,
            &["host", "/tmp", "deprecated"],
        );
    }
}

#[test]
fn rhs_connectors_prove_only_their_own_operand_and_group() {
    // Fixed CVS gives @ two touching operands under Ns, preserves a
    // final slash inside Ar, and prints a nested Op inside Pq. Neither
    // punctuation inside Ar nor an Op child can bind the separate "see".
    for (open, close, left, right) in [
        ("Op", "", '[', ']'),
        ("Oo", " Oc", '[', ']'),
        ("Pq", "", '(', ')'),
    ] {
        for (operands, text) in [
            ("Ar user Ns @ Ns Li localhost", "user@localhost"),
            ("Ar dir/", "dir/"),
            ("Ar section Ns Li .local", "section.local"),
        ] {
            let head = format!("Fl a= Ns Ar x {open} {operands}{close}");
            let label = format!("-a=x {left}{text}{right}");
            check(
                "OPTIONS",
                &head,
                &["-a"],
                &label,
                true,
                &["user", "dir", "section"],
            );
        }
    }
    for (operands, text) in [
        ("Li see Ar sections 2,3", "see sections 2,3"),
        ("Li see Op 4 | Ar mode", "see [4 | mode]"),
        ("Li see Ar section 2 Ns Li .3", "see section 2.3"),
        ("Li see Ar section 2.3", "see section 2.3"),
    ] {
        let head = format!("Fl a= Ns Ar x Pq {operands}");
        let label = format!("-a=x ({text})");
        check(
            "OPTIONS",
            &head,
            &["-a"],
            &label,
            false,
            &["see", "section", "mode"],
        );
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
fn complete_native_flag_values_keep_their_authored_symbols() {
    // termp_fl_pre() adds one dash and executes the full Fl operand.
    // Both fixed terminals and HTML keep the same spelling as typed Ar.
    for head in ["Fl std=c++17", "Fl std= Ns Ar c++17"] {
        check("OPTIONS", head, &["-std"], "-std=c++17", true, &["c++17"]);
    }
    for value in [
        "+1",
        "%s",
        "*.conf",
        "C++",
        "[+]",
        "x?y",
        "foo%20bar",
        "A#B",
    ] {
        let head = format!("Fl a={value}");
        let label = format!("-a={value}");
        check("OPTIONS", &head, &["-a"], &label, true, &[value]);
    }
}

#[test]
fn an_index_with_an_internal_pipe_remains_one_typed_parameter() {
    // In fixed CVS's tree the complete bracket spelling is one TEXT
    // child of Ar or Pa, even though the head index also exposes a pipe.
    for role in ["Ar", "Pa"] {
        for value in ["sample[old|new]", "[+|-]"] {
            let head = format!("Fl a= Ns {role} {value}");
            let label = format!("-a={value}");
            check("OPTIONS", &head, &["-a"], &label, true, &["old", "new"]);
        }
    }
}

#[test]
fn native_operand_edges_keep_connectors_inside_their_own_value() {
    // Fixed Fl, Ar, Pa and Ns execute these parts in source order.
    // Adjacent split forms retain the same exact query as whole operands.
    for (head, label, complete) in [
        ("Ar key= Ns Li auto", "-a=key=auto", true),
        (r"Ar key Ns Li \&= Ns Li auto", "-a=key=auto", true),
        ("Pa /tmp/build@ Ns Pa 2", "-a=/tmp/build@2", true),
        ("Pa /tmp/build@2", "-a=/tmp/build@2", true),
        ("Pa /tmp/build@ Pa 2", "-a=/tmp/build@ 2", false),
        ("Pa /tmp/key= Ns Pa value", "-a=/tmp/key=value", true),
        ("Ar path[ Ns Ar index ]", "-a=path[index]", true),
        ("Ar path[index]", "-a=path[index]", true),
        ("Ar x{m Ns Ar ode}", "-a=x{mode}", true),
        ("Pa x{mode Ns Ar }", "-a=x{mode}", true),
        ("Ar [dir Ns Ar /]", "-a=[dir/]", true),
        ("Pa [dir Ns Pa /]", "-a=[dir/]", true),
        ("Ar x<o Ns Ar pt>", "-a=x<opt>", true),
        ("Pa x<opt Ns Ar >", "-a=x<opt>", true),
        ("Ar x Op Ar path[ Ns Ar index ]", "-a=x [path[index]]", true),
        ("Ar x Op Ar path[index]", "-a=x [path[index]]", true),
        ("Ar x Op Ar key= Ns Li auto", "-a=x [key=auto]", true),
        (r"Ar x Op Li auto Ns Ar \&=mode", "-a=x [auto=mode]", true),
        (
            "Ar x Pq Li see Li /. Ns Pa dockerenv",
            "-a=x (see /.dockerenv)",
            false,
        ),
        (
            "Ar x Pq Li see Pa /.dockerenv",
            "-a=x (see /.dockerenv)",
            false,
        ),
        ("Ar x Pq Li /. Ns Pa dockerenv", "-a=x (/.dockerenv)", true),
        ("Ar x Pq Pa /.dockerenv", "-a=x (/.dockerenv)", true),
        ("Ar x Pq Li see Li / Ns Pa tmp", "-a=x (see /tmp)", false),
    ] {
        let head = format!("Fl a= Ns {head}");
        check(
            "OPTIONS",
            &head,
            &["-a"],
            label,
            complete,
            &["index", "see", "auto", "dockerenv"],
        );
    }
}

#[test]
fn real_assignment_forms_keep_compound_and_literal_alternatives() {
    // Reduced from node, pccritic, wavpack, dhcpcd, cmp, ld, m4, zdb
    // and exports. CVS term_word(), termp_ns_pre() and quote pre/post
    // give these tokens their boundaries. Both devices and the HTML
    // and tree profiles were checked for each reduced declaration.
    for (section, head, names, form) in [
        (
            "OPTIONS",
            "Fl -inspect-publish-uid Ns = Ns Ar stderr,http",
            &["--inspect-publish-uid"][..],
            "--inspect-publish-uid=stderr,http",
        ),
        (
            "OPTIONS",
            "Fl -define-variable Ns = Ns Ar varname Ns = Ns Ar value",
            &["--define-variable"],
            "--define-variable=varname=value",
        ),
        (
            "OPTIONS",
            "Fl -raw-pcm= Ns Ar sr , Ns Ar bits Ns [f|s|u], Ns Ar chans , Ns [le|be]",
            &["--raw-pcm"],
            "--raw-pcm=sr,bits[f|s|u],chans,[le|be]",
        ),
        // In wvunpack(1), termp_fl_pre() emits a dash before the Fl
        // text. The last bracket it opens closes after three Ar operands.
        (
            "OPTIONS",
            "Fl -skip=[-][ Ns Ar sample Ns | Ns Ar hh : Ns Ar mm : Ns Ar ss.ss ]",
            &["--skip"],
            "--skip=[-][sample|hh:mm:ss.ss]",
        ),
        (
            "OPTIONS",
            "Fl -until=[+|-][ Ns Ar sample Ns | Ns Ar hh : Ns Ar mm : Ns Ar ss.ss ]",
            &["--until"],
            "--until=[+|-][sample|hh:mm:ss.ss]",
        ),
        (
            "OPTIONS",
            "Fl Fl waitip Ns = Ns Op 4 | 6",
            &["--waitip"],
            "--waitip=[4 | 6]",
        ),
        (
            "OPTIONS",
            "Fl n Ar num , Fl -bytes= Ns num",
            &["-n", "--bytes"],
            "-n num, --bytes=num",
        ),
        ("OPTIONS", "Fl -icf Ns = Ns Cm all", &["--icf"], "--icf=all"),
        (
            "OPTIONS",
            "Fl -debug Ns = Ns Oo Oo +|- Oc Ns Ar flags Oc",
            &["--debug"],
            "--debug=[[+|-]flags]",
        ),
        (
            "OPTIONS",
            "Fl e , -exported Ns = Ns Oo Fl p Ar path Oc Ns …",
            &["-e", "--exported"],
            "-e, --exported=[-p path]…",
        ),
        (
            "OPTIONS",
            "Fl -bin Ns = Ns ( Li lsize Ns | Ns Li psize Ns | Ns Li asize )",
            &["--bin"],
            "--bin=(lsize|psize|asize)",
        ),
        (
            "OPTIONS\n.Sm off",
            r"Fl maproot Li = Ar user\^ Li \&: Ar group1\^ Li \&: group2\^ Li \&: Ar ...",
            &["-maproot"],
            "-maproot=user:group1:group2:...",
        ),
        (
            "OPTIONS\n.Sm off",
            r"Fl maproot Li = Ar user\^ Li \&: Op Ar group1\^ Li \&: group2\^ Li \&: Ar ...",
            &["-maproot"],
            "-maproot=user:[group1:group2:...]",
        ),
    ] {
        check(section, head, names, form, true, &["-p", "group1"]);
    }
}

const ASSIGNMENT_EVIDENCE_CASES: &[(&str, &str, bool)] = &[
    (
        "Fl a=application/vnd.api+json;charset=utf-8",
        "-a=application/vnd.api+json;charset=utf-8",
        true,
    ),
    ("Fl a= Ns Ar user@ Ns No host", "-a=user@host", true),
    ("Fl a= Ns Pa key= Ns No value", "-a=key=value", true),
    (
        "Fl a=text/plain;charset=utf-8",
        "-a=text/plain;charset=utf-8",
        true,
    ),
    (
        "Fl a= Ns Ar text/plain;charset=utf-8",
        "-a=text/plain;charset=utf-8",
        true,
    ),
    ("Fl a=/", "-a=/", true),
    ("Fl a= Ns Pa /", "-a=/", true),
    ("Fl a=[0-9]*", "-a=[0-9]*", true),
    ("Fl a= Ns Ar [0-9]*", "-a=[0-9]*", true),
    ("Fl a= Ns Ar x Op Ar NAME=", "-a=x [NAME=]", true),
    (
        "Fl a= Ns Ar file Oo Ar login@ Oc Ar host",
        "-a=file [login@] host",
        true,
    ),
    (
        "Fl a= Ns Ar x Op Ar NAME= Ns Li auto",
        "-a=x [NAME=auto]",
        true,
    ),
    ("Fl a= Ns Ar x Op Pa /tmp/file@", "-a=x [/tmp/file@]", true),
    (
        "Fl a= Ns Ar x Oo Pa /tmp/file@ Oc",
        "-a=x [/tmp/file@]",
        true,
    ),
    ("Fl a= Ns Ar x Pq Pa /tmp/file@", "-a=x (/tmp/file@)", true),
    ("Fl a= Ns Pa /tmp/file@", "-a=/tmp/file@", true),
    (
        "Fl a= Ns Ar x Op Pa /tmp/file@ Ns Pa main",
        "-a=x [/tmp/file@main]",
        true,
    ),
    ("Fl -require= Ns Ar pkg>=2", "--require=pkg>=2", true),
    ("Fl -filter= Ns Ar size>1024", "--filter=size>1024", true),
    ("Fl -filter= Ns Ar size<1024", "--filter=size<1024", true),
    ("Fl -filter= Ns Ar size!=1024", "--filter=size!=1024", true),
    (
        "Fl -filter= Ns Ar size<limit>",
        "--filter=size<limit>",
        true,
    ),
    ("Fl a= Ns Ar path[ Ns Ar index ]", "-a=path[index]", true),
    (
        "Fl a= Ns Ar x Pq Li see Li /. Ns Pa dockerenv",
        "-a=x (see /.dockerenv)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Li see Li ./ Ns Pa notes",
        "-a=x (see ./notes)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Li see Pa ./notes",
        "-a=x (see ./notes)",
        false,
    ),
    ("Fl a= Ns Ar x Pq Li ./ Ns Pa notes", "-a=x (./notes)", true),
    (
        "Fl a= Ns Ar x Pq Li see Li ./ Ns Pa docs@ Ns Li main",
        "-a=x (see ./docs@main)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Li see Li ../ Ns Pa docs@ Ns Li main",
        "-a=x (see ../docs@main)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Li see Pa ./docs@main",
        "-a=x (see ./docs@main)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Li ./ Ns Pa docs@ Ns Li main",
        "-a=x (./docs@main)",
        true,
    ),
    (
        "Fl a= Ns Ar x Pq Pa ./docs@main",
        "-a=x (./docs@main)",
        true,
    ),
    (
        "Fl a= Ns Ar x Pq Li see Li ./ Ns Pa docs@ Ns No main",
        "-a=x (see ./docs@main)",
        false,
    ),
];

#[test]
fn assignment_evidence_stays_inside_the_word_and_its_native_operand() {
    // CVS mdoc_term.c::termp_fl_pre and termp_ns_pre execute native words
    // whose adjacency is independent from generated enclosure closers.
    // All labels and owner boundaries were checked on the pinned devices.
    for &(head, label, complete) in ASSIGNMENT_EVIDENCE_CASES {
        let (name, _) = label.split_once('=').unwrap();
        check(
            "OPTIONS",
            head,
            &[name],
            label,
            complete,
            &["see", "notes", "main"],
        );
    }
}

#[test]
fn native_angle_spelling_cannot_swallow_an_independent_alias() {
    // The pinned tree and HTML show two Fl operands after either an
    // Ar comparison sign or an angle pair spanning two Ar operands.
    for (head, label) in [
        (
            "Fl -filter= Ns Ar size<1024 , Fl b",
            "--filter=size<1024, -b",
        ),
        (
            "Fl -filter= Ns Ar size>1024 , Fl b",
            "--filter=size>1024, -b",
        ),
        ("Fl a= Ns Ar x<o Ns Ar pt> , Fl b", "-a=x<opt>, -b"),
    ] {
        let (name, _) = label.split_once('=').unwrap();
        check(
            "OPTIONS",
            head,
            &[name, "-b"],
            label,
            true,
            &["opt", "size"],
        );
    }
}

#[test]
fn native_assignment_boundaries_respect_angle_pairs_and_independent_aliases() {
    // CVS mdoc_macro.c::in_line keeps Fl and Ar as native operands; its
    // terminal and HTML output agree on these HEAD spellings and BODY.
    for (head, label) in [
        (
            "Fl a=< Ns Ar length Ar value> , Fl b",
            "-a=<length value>, -b",
        ),
        (
            r#"Fl a=< Ns Ar "length value>" , Fl b"#,
            "-a=<length value>, -b",
        ),
        (
            r#"Fl a=< Ns Ar "length value" Ns Li > , Fl b"#,
            "-a=<length value>, -b",
        ),
        ("Fl a= Ns Ar size<1024 , Fl b", "-a=size<1024, -b"),
        ("Fl a=size<1024 , Fl b", "-a=size<1024, -b"),
        ("Fl a=/ , Fl b", "-a=/, -b"),
        ("Fl a= Ns Pa / , Fl b", "-a=/, -b"),
        ("Fl a=[0-9]* , Fl b", "-a=[0-9]*, -b"),
        (
            "Fl a=text/plain;charset=utf-8 , Fl b",
            "-a=text/plain;charset=utf-8, -b",
        ),
    ] {
        check(
            "OPTIONS",
            head,
            &["-a", "-b"],
            label,
            true,
            &["value", "length"],
        );
    }
}

#[test]
fn assignment_punctuation_uses_operand_ownership_and_group_depth() {
    // CVS mdoc_macro.c::in_line yields independent Fl operands for
    // these alias names but also for the -v inside the assigned group.
    // Both terminal devices preserve the complete visible forms.
    for (head, label, names) in [
        ("Fl a=-10,-20", "-a=-10,-20", &["-a"][..]),
        ("Fl a=first,--fake,last", "-a=first,--fake,last", &["-a"]),
        ("Fl a=-10|--fake", "-a=-10|--fake", &["-a"]),
        ("Fl a=-10,-20 , Fl b", "-a=-10,-20, -b", &["-a", "-b"]),
        ("Fl a= Ns Ar -10,-20", "-a=-10,-20", &["-a"]),
        (
            "Fl a= Ns Ar size<1024 , Fl b= Ns Ar x>4",
            "-a=size<1024, -b=x>4",
            &["-a", "-b"],
        ),
        (
            "Fl a= Ns Ar size<1024 , Fl b= Ns Ar x>4 , Fl c",
            "-a=size<1024, -b=x>4, -c",
            &["-a", "-b", "-c"],
        ),
        (
            "Fl a= Ns Ar size<1024 Op Ar x>42",
            "-a=size<1024 [x>42]",
            &["-a"],
        ),
        ("Fl a= Ns Ar size<1024 Op Ar >", "-a=size<1024 [>]", &["-a"]),
        (
            "Fl a= Ns Ar <x Ns | Ns Fl v Ns Ar > , Fl b",
            "-a=<x|-v>, -b",
            &["-a", "-b"],
        ),
        (
            "Fl a=< Ns Ar x Ns | Ns Fl v Ns Ar > , Fl b",
            "-a=<x|-v>, -b",
            &["-a", "-b"],
        ),
        (
            "Fl a= Ns Ar x<value Ns | Ns Fl v Ns Ar > , Fl b= Ns Ar x>4",
            "-a=x<value|-v>, -b=x>4",
            &["-a", "-b"],
        ),
    ] {
        check(
            "OPTIONS",
            head,
            names,
            label,
            true,
            &["-v", "-20", "--fake"],
        );
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

#[test]
fn assignment_delimiters_follow_native_operands_across_accepted_fragments() {
    // mdoc_macro.c gives each Fl an independent owner, even if Ns removes
    // the separating space. Ar typography does not close quotes or groups.
    for (head, label, names, hidden) in [
        (r"Fl a=x,\& Fl b", "-a=x, -b", &["-a", "-b"][..], &[][..]),
        (r"Fl a=x\(ba Fl b", "-a=x| -b", &["-a", "-b"][..], &[][..]),
        (
            r"Fl a=x\[u002C] Fl b",
            "-a=x, -b",
            &["-a", "-b"][..],
            &[][..],
        ),
        (
            "Fl a=x,--fake,y",
            "-a=x,--fake,y",
            &["-a"][..],
            &["--fake"][..],
        ),
        ("Fl a=x , Fl b", "-a=x, -b", &["-a", "-b"][..], &[][..]),
        (
            r"Fl a= Ns Ar x,\& Fl b",
            "-a=x, -b",
            &["-a", "-b"][..],
            &[][..],
        ),
        (
            "Fl a= Ns Ar x<1024 , Ns Fl b= Ns Ar x> , Fl c",
            "-a=x<1024,-b=x>, -c",
            &["-a", "-b", "-c"][..],
            &[][..],
        ),
        (
            "Fl a= Ns Ar x<1024 | Ns Fl b= Ns Ar x> , Fl c",
            "-a=x<1024 |-b=x>, -c",
            &["-a", "-b", "-c"][..],
            &[][..],
        ),
        (
            "Fl a= Ns Ar x<1024 , Fl b= Ns Ar x> , Fl c",
            "-a=x<1024, -b=x>, -c",
            &["-a", "-b", "-c"][..],
            &[][..],
        ),
    ] {
        check("OPTIONS", head, names, label, true, hidden);
    }
}

#[test]
fn native_groups_keep_inner_names_private_through_suffixes_and_paths() {
    for (head, label, names, hidden) in [
        (
            "Fl a= Ns Ar <x | Fl v Ns Ar >suffix , Fl b",
            "-a=<x | -v>suffix, -b",
            &["-a", "-b"][..],
            &["-v"][..],
        ),
        (
            "Fl a= Ns Ar <x | Fl v Ns Ar > Ns Ar suffix , Fl b",
            "-a=<x | -v>suffix, -b",
            &["-a", "-b"][..],
            &["-v"][..],
        ),
        (
            "Fl a= Ns Ar <x Ns | Ns Fl v Ns Ar >suffix , Fl b",
            "-a=<x|-v>suffix, -b",
            &["-a", "-b"][..],
            &["-v"][..],
        ),
        (
            "Fl a= Ns Ar x Op Ar <left Ns | Ns Fl v Ns Ar >",
            "-a=x [<left|-v>]",
            &["-a"][..],
            &["-v"][..],
        ),
        (
            "Fl a= Ns Ar x Bq Ar <left Ns | Ns Fl v Ns Ar >",
            "-a=x [<left|-v>]",
            &["-a"][..],
            &["-v"][..],
        ),
        (
            "Fl a= Ns Ar x Op Ar <left Ns | Ns Li v Ns Ar >",
            "-a=x [<left|v>]",
            &["-a"][..],
            &[][..],
        ),
        (
            r"Fl a= Ns Ar x Pq Ar value | No \&../ Ns Pa config",
            "-a=x (value | ../config)",
            &["-a"][..],
            &[][..],
        ),
        (
            "Fl a= Ns Ar x Pq Ar value | Pa ../config",
            "-a=x (value | ../config)",
            &["-a"][..],
            &[][..],
        ),
        (
            r"Fl a= Ns Ar <file Ns Li \(dq<\(dq Ns | Ns Fl v Ns Ar > , Fl b",
            r#"-a=<file"<"|-v>, -b"#,
            &["-a", "-b"][..],
            &["-v"][..],
        ),
        (
            r"Fl a= Ns Ar <file Ns Li \(dqx\(dq Ns | Ns Fl v Ns Ar > , Fl b",
            r#"-a=<file"x"|-v>, -b"#,
            &["-a", "-b"][..],
            &["-v"][..],
        ),
        (
            r"Fl a= Ns Ar <file Ns Li \(dq<\(dq | Fl v Ns Ar > , Fl b",
            r#"-a=<file"<" | -v>, -b"#,
            &["-a", "-b"][..],
            &["-v"][..],
        ),
    ] {
        check("OPTIONS", head, names, label, true, hidden);
    }
}

#[test]
fn assignment_scopes_and_alias_boundaries_follow_native_operands() {
    // CVS termp_quote_pre/post own the Aq/Bq delimiters. Its HTML and
    // terminal output agree on each It's label and independent BODY.
    for (head, names, label, full) in [
        (
            "Fl a= Ns Ar <(x>0) | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<(x>0) | -v>, -b",
            true,
        ),
        (
            "Fl a= Ns Ar <[x>0] | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<[x>0] | -v>, -b",
            true,
        ),
        (
            "Fl a= Ns Ar <(x=0) | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<(x=0) | -v>, -b",
            true,
        ),
        (
            "Fl a= Ns Ar <x>0 | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<x>0 | -v>, -b",
            true,
        ),
        (
            "Fl a= Ns Ar <x | Fl v Ns Ar >suffix , Fl b",
            &["-a", "-b"][..],
            "-a=<x | -v>suffix, -b",
            true,
        ),
        (
            "Fl a= Ns Ar x Aq Ar x<y Ns | Ns Fl v= Ns Ar y Ns Ar >",
            &["-a"][..],
            "-a=x ⟨x<y|-v=y>⟩",
            true,
        ),
        (
            "Fl a= Ns Ar x Aq Ar one | Fl v",
            &["-a"][..],
            "-a=x ⟨one | -v⟩",
            true,
        ),
        (
            "Fl a= Ns Ar x Bq Ar x<y Ns | Ns Fl v= Ns Ar y Ns Ar >",
            &["-a"][..],
            "-a=x [x<y|-v=y>]",
            true,
        ),
    ] {
        check("OPTIONS", head, names, label, full, &["-v", "--fake"]);
    }
}

#[test]
fn independent_aliases_use_the_same_cut_for_name_and_form() {
    // Both CVS devices preserve the comma and the actual operand spelling.
    for (head, names, label, full) in [
        (
            "Fl a= Ns Li x<10 , Ns Fl b= Ns Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b=y>, -c",
            true,
        ),
        (
            "Fl a= Ns Cm x<10 , Ns Fl b= Ns Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b=y>, -c",
            true,
        ),
        (
            "Fl a=x<10 , Ns Fl b= Ns Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b=y>, -c",
            true,
        ),
        (
            "Fl a= Ns Ar x<10 , Ns Fl b= Ns Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b=y>, -c",
            true,
        ),
        (
            "Fl a= Ns Pa x<10 , Ns Fl b= Ns Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b=y>, -c",
            true,
        ),
        (
            "Fl a= Ns Ar x<10 , Ns Fl b Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10,-b y>, -c",
            true,
        ),
        (
            "Fl a= Ns Ar x<10 , Fl b Ar y> , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=x<10, -b y>, -c",
            true,
        ),
        (
            r"Fl a= Ns Ar x Pq Ar value | No \&.. Ns Pa /config",
            &["-a"][..],
            "-a=x (value | ../config)",
            true,
        ),
        (
            r"Fl a= Ns Ar x Pq Ar value | No \&../ Ns Pa config",
            &["-a"][..],
            "-a=x (value | ../config)",
            true,
        ),
        (
            r"Fl a= Ns Ar x Pq Ar value | No \&.. Ns Pa config",
            &["-a"][..],
            "-a=x (value | ..config)",
            false,
        ),
    ] {
        check("OPTIONS", head, names, label, full, &["-v", "--fake"]);
    }
}

#[test]
fn command_groups_keep_their_flags_inside_the_form() {
    // CVS termp_quote_pre/post keep Aq/Op delimiters distinct from
    // the enclosed Fl words. This small source also runs from the crate package.
    let source = include_str!("grouped-command-arguments.1");
    let content = json_string_roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
    for (name, form) in [
        ("link", "link ⟨-i | -o⟩ [-a] hook head"),
        ("unlink", "unlink ⟨-i | -o⟩ hook head"),
    ] {
        let owner = owners(content.document.as_ref().unwrap())
            .into_iter()
            .find(|owner| owner.facts().is_some_and(|facts| facts.names == [name]))
            .unwrap();
        assert_eq!(
            owner
                .forms()
                .unwrap()
                .iter()
                .map(inline_plain_text)
                .collect::<Vec<_>>(),
            [form]
        );
        assert_name_positions(&content, owner, name);
    }
    for (query, count) in [("link", 1), ("unlink", 1), ("-i", 0), ("-o", 0), ("-a", 0)] {
        let reply = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: query.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(reply.counts.direct_entry.total, count, "{query}");
    }
}

const SCOPED_LITERAL_CASES: &[(&str, &[&str], &str, bool)] = &[
    (
        "Fl a= Ns Ar ket⟩ , Fl b",
        &["-a", "-b"],
        "-a=ket⟩, -b",
        true,
    ),
    (
        "Fl a= Ns Ar ket\\(ra , Fl b",
        &["-a", "-b"],
        "-a=ket⟩, -b",
        true,
    ),
    (
        "Fl a= Ns Ar ⟨psi , Fl b",
        &["-a", "-b"],
        "-a=⟨psi, -b",
        true,
    ),
    ("Fl a= Ns Ar ket , Fl b", &["-a", "-b"], "-a=ket, -b", true),
    (
        "Fl a= Ns Ar root Ao Ar x | Fl v Ac , Fl b",
        &["-a", "-b"],
        "-a=root ⟨x | -v⟩, -b",
        true,
    ),
    (
        "Fl a= Ns Ar root Ao Ar x⟩y | Fl v Ac , Fl b",
        &["-a", "-b"],
        "-a=root ⟨x⟩y | -v⟩, -b",
        true,
    ),
    (
        "Fl a= Ns Ar root Ao Li x⟩y<0 | Fl v Ac , Fl b",
        &["-a", "-b"],
        "-a=root ⟨x⟩y<0 | -v⟩, -b",
        false,
    ),
    (
        "Fl a= Ns Ar root Bo Ar x]y | Fl v Bc , Fl b",
        &["-a", "-b"],
        "-a=root [x]y | -v], -b",
        false,
    ),
    (
        "Fl a= Ns Ar <prefix>2 , Fl b= Ns Ar pkg>=2 , Fl c",
        &["-a", "-b", "-c"],
        "-a=<prefix>2, -b=pkg>=2, -c",
        true,
    ),
    (
        "Fl a= Ns Ar <prefix>2 , Fl b= Ns Ar pkg , Fl c",
        &["-a", "-b", "-c"],
        "-a=<prefix>2, -b=pkg, -c",
        true,
    ),
    (
        "Fl a= Ns Ar <prefix>suffix , Fl b= Ns Ar pkg>=2 , Fl c",
        &["-a", "-b", "-c"],
        "-a=<prefix>suffix, -b=pkg>=2, -c",
        true,
    ),
    (
        "Fl a= Ns Ar <x>0 | Fl v Ns Ar > , Fl b",
        &["-a", "-b"],
        "-a=<x>0 | -v>, -b",
        true,
    ),
    (
        "Fl a Li x<y , Fl b= Ns Ar n>=2 , Fl c",
        &["-a", "-b", "-c"],
        "-a x<y, -b=n>=2, -c",
        true,
    ),
    (
        "Fl a Cm x<y , Ns Fl b Ar y> , Fl c",
        &["-a", "-b", "-c"],
        "-a x<y,-b y>, -c",
        true,
    ),
    (
        "Fl a Ar x<y , Ns Fl b Ar y> , Fl c",
        &["-a", "-b", "-c"],
        "-a x<y,-b y>, -c",
        true,
    ),
    (
        "Fl a= Ns Ar x Pq Ar value | Li \"see the manual for x<5\"",
        &["-a"],
        "-a=x (value | see the manual for x<5)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Ar value | Cm \"see the manual for x<5\"",
        &["-a"],
        "-a=x (value | see the manual for x<5)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Ar value | Cm \"see the manual for x=5\"",
        &["-a"],
        "-a=x (value | see the manual for x=5)",
        false,
    ),
    (
        "Fl a= Ns Ar x Pq Ar value | Li x<5",
        &["-a"],
        "-a=x (value | x<5)",
        true,
    ),
    (
        "Fl a= Ns Ar x Dq Ar value | Fl v Ns Li >",
        &["-a"],
        "-a=x “value | -v>”",
        true,
    ),
    (
        "Fl a= Ns Ar x Dq Ar value | Fl v Ns Ar >",
        &["-a"],
        "-a=x “value | -v>”",
        true,
    ),
];

#[test]
fn native_enclosure_edges_and_literal_comparisons_have_distinct_roles() {
    // CVS termp_quote_pre/post own the Ao/Bo delimiter. Other literal
    // delimiters and comparisons are emitted by the operand's term_word.
    for &(head, names, label, full) in SCOPED_LITERAL_CASES {
        check("OPTIONS", head, names, label, full, &["-v", "value"]);
    }
}
