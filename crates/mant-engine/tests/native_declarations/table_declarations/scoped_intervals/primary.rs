//! Preserve primary declaration grammar across reads of the same native BODY.
use super::{HEADER, check_direct, definitions, exact_word, explain, load};
use mant_ir::{EntryInlineRoot, EntryOwner, Inline, inline_plain_text};

#[test]
fn primary_and_supplemental_enclosures_distinguish_typed_operands_from_table_cells() {
    // mdoc_term.c quote and Eo handlers lay out the actual BODY whether
    // it starts with a table or a native subject. Both occur on HEAD.
    // All 144 exact sources ran in fixed tree/ASCII/UTF-8/HTML/lint.
    for supplemental in [false, true] {
        for (open, close) in [("Bo", "Bc"), ("Po", "Pc"), ("Eo", "Ec")] {
            for (subject, key) in [
                ("Fl q", "-q"),
                ("Er EINVAL", "EINVAL"),
                ("Cm run", "run"),
                (r#"Fn checked "int value""#, "checked"),
            ] {
                for position in ["none", "before", "after", "left", "right", "both"] {
                    let source = source(open, close, subject, key, supplemental, position);
                    let content = load(&source);
                    let items = definitions(content.document.as_ref().unwrap());
                    let [item] = items.as_slice() else {
                        panic!("{source}")
                    };
                    let owner = EntryOwner::Definition(item);
                    let direct = !supplemental && (subject != "Cm run" || open == "Eo")
                        // Without a visible wrapper or table the legacy
                        // option grammar sees two independently written flags.
                        || supplemental && open == "Eo" && position == "none" && key == "-q";
                    assert_eq!(
                        explain(&content, key).counts.direct_entry.total,
                        u32::from(direct),
                        "{source}"
                    );
                    if direct {
                        check_direct(&content, key, item.source.unwrap().line, "OWNERBODY.");
                        let table_before_subject = matches!(position, "before" | "left" | "both");
                        let expected = inline_plain_text(&item.terms[0])
                            .match_indices(key)
                            .nth(usize::from(table_before_subject))
                            .unwrap()
                            .0;
                        assert_eq!(binding_start(owner, key), expected, "{source}");
                    }
                    if supplemental {
                        check_direct(&content, "-p", item.source.unwrap().line, "OWNERBODY.");
                    }
                    let count = match position {
                        "none" => 0,
                        "both" => 2,
                        _ => 1,
                    };
                    assert_eq!(exact_word(&content, "SENTINEL"), count, "{source}");
                    assert_eq!(
                        explain(&content, "SENTINEL").counts.direct_entry.total,
                        0,
                        "{source}"
                    );
                    if let Some(forms) = owner.forms() {
                        assert!(
                            forms
                                .iter()
                                .all(|form| !inline_plain_text(form).contains("SENTINEL")),
                            "{source}"
                        );
                    }
                }
            }
        }
    }
}

pub(super) fn binding_start(owner: EntryOwner<'_>, name: &str) -> usize {
    let EntryOwner::Definition(item) = owner else {
        panic!("expected a definition owner")
    };
    let facts = owner.facts().unwrap();
    let name_index = facts
        .names
        .iter()
        .position(|candidate| candidate == name)
        .unwrap();
    let binding = facts
        .name_bindings
        .iter()
        .find(|binding| binding.name == name_index)
        .unwrap();
    let [occurrence] = binding.occurrences.as_slice() else {
        panic!("expected exactly one source occurrence: {binding:?}")
    };
    let part = &occurrence.parts[0];
    let EntryInlineRoot::Term { index } = part.root else {
        panic!("expected a range in the definition term: {part:?}")
    };
    let mut inlines = item.terms[index].content.as_slice();
    let mut start = part.bytes.as_ref().map_or(0, |bytes| bytes.start);
    for &index in &part.path {
        start += inline_plain_text(&inlines[..index]).len();
        inlines = match &inlines[index] {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => children,
            _ => &[],
        };
    }
    start
}

fn source(
    open: &str,
    close: &str,
    subject: &str,
    key: &str,
    supplemental: bool,
    position: &str,
) -> String {
    let table = format!(".TS\nl.\nSENTINEL\n{key}\n.TE\n");
    let prefix = if supplemental { ".Fl p\n" } else { "" };
    let table_at = |present| if present { table.as_str() } else { "" };
    let before = table_at(position == "before");
    let left = table_at(matches!(position, "left" | "both"));
    let right = table_at(matches!(position, "right" | "both"));
    let after = table_at(position == "after");
    format!(
        "{HEADER}.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Xo\n{prefix}{before}.{open}\n{left}.{subject}\n{right}.{close}\n{after}.Xc\nOWNERBODY.\n.El\n"
    )
}
