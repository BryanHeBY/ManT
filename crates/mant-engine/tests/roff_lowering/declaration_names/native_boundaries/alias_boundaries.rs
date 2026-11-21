//! Independent aliases and command names follow native scope boundaries.

use super::{
    ExplanationOptions, ExplanationQuery, assert_name_positions, check, inline_plain_text,
    json_string_roundtrip, owners,
};

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
    let source = include_str!("../grouped-command-arguments.1");
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

// The mdoc enclosure handlers write delimiters independently of Fl/Ar
// operands. Both literal angle families must keep the same scoped roles.

#[test]
fn literal_angle_values_and_native_enclosures_keep_independent_names() {
    for (head, names, form, hidden) in [
        (
            r"Fl a= Ns Ar ⟨yes | Fl v Ns Ar ⟩ , Fl b",
            &["-a", "-b"][..],
            "-a=⟨yes | -v⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar \(layes | Fl v Ns Ar \(ra , Fl b",
            &["-a", "-b"][..],
            "-a=⟨yes | -v⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar <yes | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<yes | -v>, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar root Ao Ar yes | Fl v Ac , Fl b",
            &["-a", "-b"][..],
            "-a=root ⟨yes | -v⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a=⟨root⟩ , Fl b",
            &["-a", "-b"][..],
            "-a=⟨root⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a=\(laroot\(ra , Fl b",
            &["-a", "-b"][..],
            "-a=⟨root⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar ⟨root⟩ , Fl b",
            &["-a", "-b"][..],
            "-a=⟨root⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a=<root> , Fl b",
            &["-a", "-b"][..],
            "-a=<root>, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar ket⟩ , Fl b",
            &["-a", "-b"][..],
            "-a=ket⟩, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar ⟨root , Fl b",
            &["-a", "-b"][..],
            "-a=⟨root, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar <min>2 | Fl b= Ns Ar max>=3 , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=<min>2 | -b=max>=3, -c",
            &["-v"][..],
        ),
        (
            r"Fl a= Ns Ar <min>2 , Fl b Ar max>=3 , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=<min>2, -b max>=3, -c",
            &["-v"][..],
        ),
        (
            r"Fl a= Ns Ar <min>2 , Fl b= Ns Ar max>=3 , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=<min>2, -b=max>=3, -c",
            &["-v"][..],
        ),
        (
            r"Fl a= Ns Ar <min>2 | Fl v Ns Ar > , Fl b",
            &["-a", "-b"][..],
            "-a=<min>2 | -v>, -b",
            &["-v", "-c"][..],
        ),
        (
            r"Fl a= Ns Ar <min>2 | Fl b= Ns Ar max , Fl c",
            &["-a", "-b", "-c"][..],
            "-a=<min>2 | -b=max, -c",
            &["-v"][..],
        ),
    ] {
        check("OPTIONS", head, names, form, true, hidden);
    }
}
