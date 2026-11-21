//! Native operands prove complete assignments and their accepted forms.

use super::check;

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
