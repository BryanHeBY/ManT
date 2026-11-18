//! Quoted operand spaces contribute to the original term even at field edges.

use std::fmt::Write as _;

use mant_ir::{Block, ListKind};

use super::{MAN, MDOC, paragraph};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    It,
    Ip,
    Tp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Spaces {
    Syntax,
    Leading,
    Trailing,
    Fixed,
}

impl Spaces {
    fn operand(self, number: u8, active: bool) -> String {
        if !active {
            return format!("{number}.");
        }
        match self {
            Self::Syntax => format!("        {number}."),
            Self::Leading => format!("\"        {number}.\""),
            Self::Trailing => format!("\"{number}.        \""),
            Self::Fixed => format!(r"\~{number}."),
        }
    }

    fn label(self, number: u8, active: bool) -> String {
        if !active {
            return format!("{number}.");
        }
        match self {
            Self::Syntax => format!("{number}."),
            Self::Leading => format!("        {number}."),
            Self::Trailing => format!("{number}.        "),
            Self::Fixed => format!("\u{a0}{number}."),
        }
    }
}

#[test]
fn ordinal_replacement_retains_executed_operand_spaces() {
    // Fixed CVS print_man_node()/print_mdoc_node() pass the unquoted TEXT
    // argument to term_word(). Spaces between a macro and its operand are
    // syntax, while the quoted ones enter term_fill's layout calculations.
    for kind in [Kind::It, Kind::Ip, Kind::Tp] {
        for spaces in [
            Spaces::Syntax,
            Spaces::Leading,
            Spaces::Trailing,
            Spaces::Fixed,
        ] {
            for numbers in [[1, 2, 3], [1, 4, 5]] {
                for mask in 0_u8..8 {
                    check(kind, spaces, numbers, mask);
                }
            }
        }
    }
}

fn check(kind: Kind, spaces: Spaces, numbers: [u8; 3], mask: u8) {
    let mut source = if kind == Kind::It {
        MDOC.replace("exercise descriptors", "probe page") + ".Bl -tag -width Ds\n"
    } else {
        MAN.to_owned()
    };
    for (index, number) in numbers.into_iter().enumerate() {
        let label = spaces.operand(number, mask & (1 << index) != 0);
        let head = match kind {
            Kind::It => format!(".It {label}"),
            Kind::Ip => format!(".IP {label} 4"),
            Kind::Tp => format!(".TP 4\n.B {label}"),
        };
        writeln!(source, "{head}\nBODY{}", index + 1).unwrap();
    }
    if kind == Kind::It {
        source.push_str(".El\n");
    }

    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let query = crate::json_roundtrip::json_string_roundtrip(&query);
    let doc = query.document.as_ref().unwrap();
    let blocks = &doc.sections[usize::from(kind == Kind::It)].blocks;
    let mut owners = Vec::new();
    for block in blocks {
        match block {
            Block::List {
                kind: ListKind::Ordered { start },
                items,
                ..
            } => {
                let numbering = ListKind::Ordered { start: *start };
                owners.extend(items.iter().enumerate().map(|(index, item)| {
                    (
                        numbering.ordinal(index),
                        None,
                        mant_ir::inline_plain_text(paragraph(&item.blocks)),
                    )
                }));
            }
            Block::DefinitionList { items, .. } => owners.extend(items.iter().map(|item| {
                (
                    None,
                    Some(mant_ir::inline_plain_text(&item.terms[0])),
                    mant_ir::inline_plain_text(paragraph(&item.description)),
                )
            })),
            _ => panic!("{source}\n{block:?}"),
        }
    }
    assert_eq!(owners.len(), 3, "{source}");
    let rendered = mant_render::render_query_man_at_width(&query, 78);
    let rows: Vec<_> = rendered.lines().map(str::trim_ascii).collect();
    let padded = spaces != Spaces::Syntax && mask != 0;
    for (index, ((marker, label, body), number)) in owners.into_iter().zip(numbers).enumerate() {
        let active = mask & (1 << index) != 0;
        let uses_generated = if kind == Kind::It {
            !padded && numbers[1] == numbers[0] + 1
        } else {
            !active || spaces == Spaces::Syntax
        };
        assert_eq!(
            marker,
            uses_generated.then_some(u64::from(number)),
            "{source}"
        );
        assert_eq!(
            label,
            (!uses_generated).then(|| spaces.label(number, active)),
            "{source}"
        );
        assert_eq!(body, format!("BODY{}", index + 1), "{source}");
        let body_row = rows.iter().position(|row| row.ends_with(&body)).unwrap();
        // CVS term_fill() uses trailing cells in the scan even when
        // term_field() does not emit them. TP/tag and IP have different
        // NOBREAK flags, so only TP/tag split on the trailing-space lane.
        let marker = format!("{number}.");
        let split =
            active && (spaces == Spaces::Leading || spaces == Spaces::Trailing && kind != Kind::Ip);
        if split {
            assert_eq!(
                &rows[body_row - 1..=body_row],
                &[marker.as_str(), body.as_str()],
                "{source}\n{rendered}"
            );
        } else {
            assert!(rows[body_row].contains(&marker), "{source}\n{rendered}");
        }
    }
}
