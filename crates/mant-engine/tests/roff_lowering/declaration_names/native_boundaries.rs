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
