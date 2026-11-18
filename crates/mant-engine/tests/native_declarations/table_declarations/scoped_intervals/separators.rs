//! Declared alternatives and annotations span display-only rows without sharing names.
use super::{HEADER, definitions, exact_word, explain, load, primary::binding_start};
use crate::support::explanation_ranges::assert_ranges;
use mant_ir::{DefinitionItem, EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

struct Declarations<'a> {
    section: &'a str,
    first: (&'a str, &'a str),
    second: (&'a str, &'a str),
}

const ERROR: Declarations<'_> = Declarations {
    section: "DESCRIPTION",
    first: ("Er EFAULT", "EFAULT"),
    second: ("Er EINVAL", "EINVAL"),
};
const OPTION: Declarations<'_> = Declarations {
    section: "OPTIONS",
    first: ("Fl a", "-a"),
    second: ("Fl b", "-b"),
};
const COMMAND: Declarations<'_> = Declarations {
    section: "COMMANDS",
    first: ("Cm first", "first"),
    second: ("Cm second", "second"),
};
const ENVIRONMENT: Declarations<'_> = Declarations {
    section: "ENVIRONMENT",
    first: ("Ev FIRST", "FIRST"),
    second: ("Ev SECOND", "SECOND"),
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Placement {
    Absent,
    Before,
    BeforeSeparator,
    AfterSeparator,
    InnerBefore,
    InnerAfter,
    After,
}

const PLACEMENTS: [Placement; 7] = [
    Placement::Absent,
    Placement::Before,
    Placement::BeforeSeparator,
    Placement::AfterSeparator,
    Placement::InnerBefore,
    Placement::InnerAfter,
    Placement::After,
];

#[test]
fn an_explicit_separator_starts_a_second_native_declaration_after_display() {
    // mdoc_macro.c::blk_part_exp opens a fresh native BODY after the
    // independently executed No. Fixed CVS tree, HTML, ASCII, UTF-8 and
    // lint all ran on the exact sources, including repeated cell spelling.
    for declarations in [ERROR, OPTION, COMMAND, ENVIRONMENT] {
        for wrapper in [("", ""), ("Eo", "Ec"), ("Bo", "Bc"), ("Po", "Pc")] {
            for separator in ["", r"No \&,", r"No \&|"] {
                for position in PLACEMENTS {
                    if wrapper.0.is_empty()
                        && matches!(position, Placement::InnerBefore | Placement::InnerAfter)
                        || separator.is_empty() && position == Placement::BeforeSeparator
                    {
                        continue;
                    }
                    let source = declarations.source(wrapper, separator, position);
                    let content = load(&source);
                    let [first, second] =
                        *definitions(content.document.as_ref().unwrap()).as_slice()
                    else {
                        panic!("{source}")
                    };
                    let expected = declarations.declared_names(wrapper.0, separator, position);
                    declarations.assert_result(
                        &content,
                        [first, second],
                        expected,
                        position,
                        &source,
                    );
                }
            }
        }
    }
}

#[test]
fn table_seams_retain_complete_alternatives_without_naming_the_connector() {
    let literal = ("No FIRST", "FIRST");
    let native = ("Ev SECOND", "SECOND");
    for (declarations, separator, admitted) in [
        (ERROR, "No or", false),
        (OPTION, "No or", true),
        (
            Declarations {
                first: literal,
                second: native,
                ..ENVIRONMENT
            },
            r"No \&,",
            true,
        ),
        (
            Declarations {
                first: native,
                second: literal,
                ..ENVIRONMENT
            },
            r"No \&|",
            true,
        ),
        (ENVIRONMENT, "No or", true),
    ] {
        for position in PLACEMENTS {
            if matches!(position, Placement::InnerBefore | Placement::InnerAfter) {
                continue;
            }
            let source = declarations.source(("", ""), separator, position);
            let content = load(&source);
            assert_eq!(
                explain(&content, "or").counts.direct_entry.total,
                0,
                "{source}"
            );
            if admitted {
                let [first, second] = *definitions(content.document.as_ref().unwrap()).as_slice()
                else {
                    panic!("{source}")
                };
                declarations.assert_result(
                    &content,
                    [first, second],
                    [true, true],
                    position,
                    &source,
                );
            }
        }
    }
}

#[test]
fn punctuation_inside_prose_cannot_restart_a_native_declaration() {
    // mdoc_macro.c::dword and append_delims separate a whole bare delimiter
    // from the previous No operand. An escape inside one TEXT is still prose.
    for (note, separated) in [
        (r#"No "a note\&,""#, false),
        (r#"No "a note\&|""#, false),
        (r#"No "a note\[u002C]""#, false),
        (r#"No "a note\[u007C]""#, false),
        (r#"No "a note" ,"#, true),
        (r#"No "a note" |"#, true),
    ] {
        let source = format!(
            "{HEADER}.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Xo\n.Er EFAULT\n.{note}\n.TS\nl.\nSENTINEL\nEINVAL\n.TE\n.Eo\n.Er EINVAL\n.Ec\n.Xc\nOWNERBODY.\n.It Er EINVAL\nDECOYBODY.\n.El\n"
        );
        let content = load(&source);
        let [item, decoy] = *definitions(content.document.as_ref().unwrap()).as_slice() else {
            panic!("{source}");
        };
        let mut sources = vec![(decoy, "DECOYBODY.")];
        if separated {
            let owner = EntryOwner::Definition(item);
            let term = inline_plain_text(&item.terms[0]);
            assert_eq!(
                binding_start(owner, "EINVAL"),
                term.rfind("EINVAL").unwrap(),
                "{source}"
            );
            sources.push((item, "OWNERBODY."));
        }
        assert_query(&content, "EINVAL", &sources, &source);
        assert_eq!(exact_word(&content, "SENTINEL"), 1, "{source}");
        assert_query(&content, "SENTINEL", &[], &source);
    }
}

#[test]
fn a_rejected_prefix_cannot_suppress_a_later_independent_name() {
    // in_line() creates the last Er independently of the rejected Er or
    // supplemental Po BODY. Fixed CVS preserves both exact source owners.
    for separator in [r".No \&,", r".No \&|"] {
        for middle in [".No or\n.Er EINVAL", ".Po\n.Er EINVAL\n.Pc\n.No staging"] {
            let source = format!(
                "{HEADER}.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Xo\n.Er EFAULT\n.TS\nl.\nSENTINEL\n.TE\n{middle}\n{separator}\n.Er EACCES\n.Xc\nOWNERBODY.\n.It Er EACCES\nDECOYBODY.\n.El\n"
            );
            let content = load(&source);
            let [owner, decoy] = *definitions(content.document.as_ref().unwrap()).as_slice() else {
                panic!("{source}");
            };
            assert_query(&content, "EFAULT", &[(owner, "OWNERBODY.")], &source);
            assert_query(
                &content,
                "EACCES",
                &[(owner, "OWNERBODY."), (decoy, "DECOYBODY.")],
                &source,
            );
            for rejected in ["or", "EINVAL", "SENTINEL"] {
                assert_query(&content, rejected, &[], &source);
            }
        }
    }
}

impl Declarations<'_> {
    fn source(&self, (open, close): (&str, &str), separator: &str, position: Placement) -> String {
        let table = format!(".TS\nl.\nSENTINEL\n{}\n.TE\n", self.second.1);
        let at = |expected| {
            if position == expected {
                table.as_str()
            } else {
                ""
            }
        };
        let request = |value: &str| {
            if value.is_empty() {
                String::new()
            } else {
                format!(".{value}\n")
            }
        };
        format!(
            "{HEADER}.Sh {}\n.Bl -tag -width Ds\n.It Xo\n{}.{}\n{}{}{}{}{}.{}\n{}{}{}.Xc\nOWNERBODY.\n.It {}\nDECOYBODY.\n.El\n",
            self.section,
            at(Placement::Before),
            self.first.0,
            at(Placement::BeforeSeparator),
            request(separator),
            at(Placement::AfterSeparator),
            request(open),
            at(Placement::InnerBefore),
            self.second.0,
            at(Placement::InnerAfter),
            request(close),
            at(Placement::After),
            self.second.0,
        )
    }

    fn declared_names(&self, open: &str, separator: &str, position: Placement) -> [bool; 2] {
        let visible = matches!(open, "Bo" | "Po");
        if !separator.is_empty() {
            // Er's existing grammar admits a complete second bounded
            // subject. The other families only prove the transparent Eo
            // or unwrapped alternative in this declaration shape.
            return [true, !visible || self.section == ERROR.section];
        }
        // A top-level display can split two unbounded literal intervals.
        // A native BODY still owns its contents when no separator precedes it.
        if open.is_empty() && position == Placement::AfterSeparator
            || self.section == OPTION.section
                && (open.is_empty() || open == "Eo" && position == Placement::Absent)
        {
            return [true, true];
        }
        let recognized_first = match position {
            Placement::Absent => visible && self.section != ERROR.section,
            Placement::Before | Placement::After if open.is_empty() => false,
            _ => true,
        };
        [recognized_first, false]
    }

    fn assert_result(
        &self,
        content: &ResolvedContent,
        [item, decoy]: [&DefinitionItem; 2],
        [declared_first, declared_second]: [bool; 2],
        position: Placement,
        source: &str,
    ) {
        let owner = EntryOwner::Definition(item);
        let expected: Vec<_> = [
            (self.first.1, declared_first),
            (self.second.1, declared_second),
        ]
        .into_iter()
        .filter_map(|(name, declared)| declared.then_some(name))
        .collect();
        let names = owner.validated_names().unwrap_or_default();
        // The ordinary command grammar can retain the exact joined or
        // parenthesized spelling. The two individual tokens cannot borrow
        // those bindings, regardless of the display position.
        let literal_command = |name: &str| {
            self.section == COMMAND.section && matches!(name, "first second" | "(second)")
        };
        assert_eq!(
            names
                .iter()
                .map(String::as_str)
                .filter(|name| !literal_command(name))
                .collect::<Vec<_>>(),
            expected,
            "{source}"
        );
        let visible = inline_plain_text(&item.terms[0]);
        for (name, is_second) in [(self.first.1, false), (self.second.1, true)] {
            let mut sources = Vec::new();
            if expected.contains(&name) {
                sources.push((item, "OWNERBODY."));
                let before_subject = is_second
                    && matches!(
                        position,
                        Placement::Before
                            | Placement::BeforeSeparator
                            | Placement::AfterSeparator
                            | Placement::InnerBefore
                    );
                let (start, _) = visible
                    .match_indices(name)
                    .nth(usize::from(before_subject))
                    .unwrap();
                assert_eq!(binding_start(owner, name), start, "{source}");
            }
            if is_second {
                sources.push((decoy, "DECOYBODY."));
            }
            assert_query(content, name, &sources, source);
        }
        if position == Placement::Absent {
            if declared_first && declared_second {
                let complete = visible.trim();
                assert!(
                    owner
                        .forms()
                        .unwrap()
                        .iter()
                        .any(|form| inline_plain_text(form) == complete),
                    "{source}"
                );
            }
        } else {
            assert_eq!(exact_word(content, "SENTINEL"), 1, "{source}");
            assert_query(content, "SENTINEL", &[], source);
            assert!(
                owner.forms().is_none_or(|forms| forms
                    .iter()
                    .all(|form| !inline_plain_text(form).contains("SENTINEL"))),
                "{source}"
            );
        }
    }
}

pub(super) fn assert_query(
    content: &ResolvedContent,
    name: &str,
    sources: &[(&DefinitionItem, &str)],
    source: &str,
) {
    let result = explain(content, name);
    let actual: Vec<_> = result
        .evidence
        .iter()
        .filter(|evidence| evidence.class == EvidenceClass::DirectEntry)
        .collect();
    assert_eq!(actual.len(), sources.len(), "{name}\n{source}");
    assert_eq!(
        result.counts.direct_entry.total as usize,
        sources.len(),
        "{name}\n{source}"
    );
    if !sources.is_empty() {
        assert!(
            assert_ranges(&result, name) >= sources.len(),
            "{name}\n{source}"
        );
    }
    for (item, sentinel) in sources {
        let hit = actual
            .iter()
            .find(|hit| hit.source == item.source)
            .unwrap_or_else(|| panic!("{name} at {sentinel}\n{source}"));
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. })),
            "{name}\n{source}"
        );
        let excerpt =
            mant_query::select_excerpt(content, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        let rendered = mant_render::render_excerpt_text(&excerpt);
        assert!(rendered.contains(sentinel), "{name}\n{source}\n{rendered}");
    }
}
