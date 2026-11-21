//! Native assignment groups protect their own operands and separators.

use super::check;

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
