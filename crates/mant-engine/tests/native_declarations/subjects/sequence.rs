use super::super::support::{check_occurrences, explain, load, roundtrip};
use mant_ir::{EntryKind, EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::{ContentSelector, EntryProjection, EvidenceClass, OutlineNode};

mod annotations;
mod receipts;
mod symbols;

fn fixture(head: &str, space_off: bool) -> ResolvedContent {
    let spacing = if space_off { ".Sm off\n" } else { "" };
    load(&format!(
        ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n.Sh DESCRIPTION\n{spacing}.Bl -tag -width Ds\n.It {head}\nOWNERBODY.\n.El\n"
    ))
}

fn check(
    head: &str,
    space_off: bool,
    kind: EntryKind,
    names: &[&str],
    form: &str,
    arguments: &[&str],
) {
    let query = fixture(head, space_off);
    let items = super::super::definitions(query.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{head}");
    };
    let owner = EntryOwner::Definition(item);
    assert_eq!(owner.facts().unwrap().kind, kind, "{head}");
    assert_eq!(owner.validated_names().unwrap(), names, "{head}");
    assert_eq!(inline_plain_text(&item.terms[0]), form, "{head}");

    let outline = roundtrip(
        &mant_query::build_outline_projection(&query, EntryProjection::All, None).unwrap(),
    );
    let [
        entry @ OutlineNode::DocumentEntry {
            names: indexed,
            forms,
            ..
        },
    ] = outline.nodes.last().unwrap().children()
    else {
        panic!("one owner for {head}");
    };
    assert_eq!(indexed, names, "{head}");
    assert_eq!(forms.as_slice(), [form], "{head}");
    let excerpt = roundtrip(
        &mant_query::select_excerpt(&query, &[ContentSelector::path(entry.path())]).unwrap(),
    );
    assert_eq!(
        excerpt.selections[0].outline().path(),
        entry.path(),
        "{head}"
    );
    assert!(
        mant_render::render_excerpt_text(&excerpt).contains("OWNERBODY."),
        "{head}"
    );
    for name in names {
        let result = roundtrip(&explain(&query, name));
        assert_eq!(result.counts.direct_entry.total, 1, "{name}: {head}");
        let hit = result
            .evidence
            .iter()
            .find(|hit| hit.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, item.source, "{head}");
        assert_eq!(hit.outline.path(), entry.path(), "{head}");
        let offset = form.find(name).unwrap();
        let start = form[..offset].chars().count();
        let range = start..start + name.chars().count();
        check_occurrences(hit, name, std::slice::from_ref(&range));

        assert_eq!(excerpt.selections[0].outline(), &hit.outline, "{head}");
    }
    for argument in arguments {
        assert_eq!(
            explain(&query, argument).counts.direct_entry.total,
            0,
            "{argument}: {head}"
        );
    }
}

#[test]
fn argument_tails_keep_their_roles_across_mixed_prefixes_and_enclosures() {
    const SEARCH: &str = r"Bq Sy \z/ Ar RE";
    // CVS mdoc_term.c::termp_quote_pre/post emits the Bo/Po BODY fence;
    // -Ttree puts the comma after Bc/Pc outside that native scope.
    for space_off in [false, true] {
        let gap = if space_off { "," } else { ", " };
        let wrapped = if space_off {
            "[RE,START]"
        } else {
            "[/RE, START]"
        };
        for (prefix, names, start) in [
            ("Dv START ,".to_owned(), &["START"][..], "START".to_owned()),
            (
                "Va ONE , Dv START ,".to_owned(),
                &["ONE", "START"][..],
                format!("ONE{gap}START"),
            ),
            (
                format!("{SEARCH} , Dv START ,"),
                &["START"][..],
                wrapped.to_owned(),
            ),
        ] {
            for (argument, printed) in [
                (".Fa arg ,", "arg"),
                (".Bo\n.Fa arg\n.Bc ,", "[arg]"),
                (".Po\n.Fa arg\n.Pc ,", "(arg)"),
            ] {
                check(
                    &format!("Xo {prefix}\n{argument}\n.Dv STOP\n.Xc"),
                    space_off,
                    EntryKind::Term,
                    names,
                    &format!("{start}{gap}{printed}{gap}STOP"),
                    &["RE", "arg", "STOP"],
                );
            }
        }
    }
    check(
        &format!("{SEARCH} , Dv START , Fa arg , Dv STOP"),
        true,
        EntryKind::Term,
        &["START"],
        "[RE,START,arg,STOP]",
        &["RE", "arg", "STOP"],
    );
    check(
        &format!("{SEARCH} , Dv START , No DECORATIVE , Fa arg , Dv STOP"),
        false,
        EntryKind::Term,
        &["START"],
        "[/RE, START, DECORATIVE, arg, STOP]",
        &["RE", "DECORATIVE", "arg", "STOP"],
    );
}

#[test]
fn mixed_native_subjects_keep_source_order_in_both_directions() {
    for (head, names, form) in [
        ("Va var , Dv START", ["var", "START"], "var,START"),
        ("Dv START , Va var", ["START", "var"], "START,var"),
        ("Dv FIRST , Er EINVAL", ["FIRST", "EINVAL"], "FIRST,EINVAL"),
        ("Er EINVAL , Dv FIRST", ["EINVAL", "FIRST"], "EINVAL,FIRST"),
        ("Dv FIRST , Dv SECOND", ["FIRST", "SECOND"], "FIRST,SECOND"),
    ] {
        check(head, true, EntryKind::Term, &names, form, &[]);
    }
}

#[test]
fn independently_closed_enclosures_do_not_extend_a_parameter_tail() {
    for (head, names, form, not_names) in [
        (
            "Xo\n.Bo\n.Dv START ,\n.Fa arg ,\n.Dv STOP\n.Bc ,\n.Va outside\n.Xc",
            &["START", "outside"][..],
            "[START, arg, STOP], outside",
            &["arg", "STOP"][..],
        ),
        (
            "Xo\n.Bo\n.Dv START\n.Bc ,\n.Fa arg ,\n.Dv STOP\n.Xc",
            &["START"],
            "[START], arg, STOP",
            &["arg", "STOP"],
        ),
        (
            "Xo\n.Dv START ,\n.Bo\n.Po\n.Fa arg\n.Pc\n.Bc ,\n.Dv STOP\n.Xc",
            &["START"],
            "START, [(arg)], STOP",
            &["arg", "STOP"],
        ),
        (
            "Xo\n.Dv START\n.Bo\n.Fa arg\n.Bc ,\n.Dv ZERO\n.Xc",
            &["START", "ZERO"],
            "START [arg], ZERO",
            &["arg"],
        ),
        (
            "Xo\n.Dv START ,\n.Bo\n.No arg\n.Bc ,\n.Dv STOP\n.Xc",
            &["START", "STOP"],
            "START, [arg], STOP",
            &["arg"],
        ),
        (
            "Xo\n.Bo\n.Dv START\n.Bc ,\n.Po\n.Va outside\n.Pc\n.Xc",
            &["START", "outside"],
            "[START], (outside)",
            &[],
        ),
        (
            r#"Va "ONE, TWO" , Dv START"#,
            &["START"],
            "ONE, TWO, START",
            &["ONE", "TWO"],
        ),
    ] {
        check(head, false, EntryKind::Term, names, form, not_names);
    }
}

#[test]
fn mixed_subjects_use_the_same_name_budget_without_losing_the_owner() {
    use std::fmt::Write as _;

    for count in [256, 257] {
        let mut head = "Xo\n".to_owned();
        let names = (0..count)
            .map(|index| format!("ITEM_{index}"))
            .collect::<Vec<_>>();
        for (index, name) in names.iter().enumerate() {
            let macro_name = if index % 2 == 0 { "Dv" } else { "Va" };
            let comma = if index + 1 < count { " ," } else { "" };
            writeln!(head, ".{macro_name} {name}{comma}").unwrap();
        }
        head.push_str(".Xc");
        let query = fixture(&head, false);
        let document = query.document.as_ref().unwrap();
        let items = super::super::definitions(document);
        let [item] = items.as_slice() else {
            panic!("{count} operands");
        };
        let owner = EntryOwner::Definition(item);
        assert_eq!(inline_plain_text(&item.terms[0]), names.join(", "));
        assert_eq!(
            owner.validated_names().unwrap(),
            if count == 256 { names.as_slice() } else { &[] }
        );
        assert_eq!(
            document
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code.as_deref()
                    == Some("manual.semantic-entry.name-limit"))
                .count(),
            usize::from(count == 257)
        );
        let outline = roundtrip(
            &mant_query::build_outline_projection(&query, EntryProjection::All, None).unwrap(),
        );
        let [entry] = outline.nodes.last().unwrap().children() else {
            panic!("{count} operands");
        };
        assert_eq!(entry.path(), "2/e1");
        let id = ContentSelector::id(entry.id());
        id.validate().expect("published ID is a valid selector");
        let excerpt = mant_query::select_excerpt(&query, &[id]).unwrap();
        assert_eq!(
            excerpt,
            mant_query::select_excerpt(&query, &[ContentSelector::path(entry.path())]).unwrap(),
        );
        assert!(mant_render::render_excerpt_text(&excerpt).contains("OWNERBODY."));
    }
}
