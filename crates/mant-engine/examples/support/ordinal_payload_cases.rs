//! Fixed-CVS evidence for display payloads and ordinal HEAD controls.

use super::super::OrdinalDisposition::{Recovered, Retained};
use super::execution::Case;
use mant_ir::{Block, Document, inline_plain_text};

const HEADER: &str = concat!(
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n",
    ".Nd test page\n.Sh DESCRIPTION\n"
);
const FOOTER: &str = ".Sh END PROBE\nDone.\n";

pub(crate) struct DisplayCase {
    pub(crate) case: Case,
    pub(crate) ordinal: bool,
    // Each profiler compiles the same coordinate matrix but measures its
    // own independent evidence.
    pub(crate) literal_count: usize,
    text: &'static str,
    empty_row: bool,
}

impl DisplayCase {
    pub(crate) fn assert_body(&self, document: &Document) {
        let blocks = &document.sections[1].blocks;
        let name = &self.case.name;
        let slots: Vec<&[Block]> = if self.ordinal {
            match blocks.as_slice() {
                [Block::List { items, .. }] if self.case.disposition == Recovered => {
                    items.iter().map(|item| item.blocks.as_slice()).collect()
                }
                [Block::DefinitionList { items, .. }] if self.case.disposition == Retained => items
                    .iter()
                    .map(|item| item.description.as_slice())
                    .collect(),
                _ => panic!("{name}: unexpected list owner {blocks:?}"),
            }
        } else {
            vec![&blocks[1..blocks.len() - 1]]
        };
        assert_eq!(slots.len(), 1 + usize::from(self.ordinal), "{name}");
        for slot in slots {
            let slot = slot
                .iter()
                .filter(|block| {
                    !matches!(block, Block::Paragraph { children, .. }
                    if inline_plain_text(children).is_empty())
                })
                .collect::<Vec<_>>();
            match slot.as_slice() {
                [Block::VerticalSpace { lines: 1, .. }] if self.empty_row => {}
                [Block::Preformatted { children, .. }] if self.literal_count > 0 => {
                    assert_eq!(inline_plain_text(children), self.text, "{name}");
                }
                [Block::Paragraph { children, .. }] if !self.text.is_empty() => {
                    assert_eq!(self.literal_count, 0, "{name}");
                    assert_eq!(inline_plain_text(children), self.text, "{name}");
                }
                [] if self.text.is_empty() && !self.empty_row => {}
                _ => panic!("{name}: unexpected owned display payload {slot:?}"),
            }
        }
    }
}

pub(crate) fn displays() -> impl Iterator<Item = DisplayCase> {
    // termp_d1_pre/termp_bl_post and termp_bd_pre/post close a display.
    // A compact display writes nothing until the body buffers a cell.
    // term_fill() can complete a physical row without printing a glyph.
    let forms = [
        ("literal", "PROOF", "PROOF", false),
        ("motion", r"PROOF\h'0n'", "PROOF", false),
        ("overstrike", r"PROOF\o'AB'", "PROOFB", false),
        ("font", r"\fB", "", false),
        ("continue", r"\c", "", false),
        ("empty-row", r"\&", "", true),
        ("breakpoint", r"\:", "", true),
        ("marker", r"\p", "", true),
        ("ignored", ".Db GHOSTTOKEN", "", false),
    ];
    ["D1", "Dl", "literal", "unfilled", "filled", "centered"]
        .into_iter()
        .flat_map(move |mode| {
            let short = matches!(mode, "D1" | "Dl");
            let literal = short || matches!(mode, "literal" | "unfilled");
            forms
                .into_iter()
                .filter(move |(kind, ..)| *kind != "ignored" || !short)
                .flat_map(move |(kind, operand, text, empty_row)| {
                    [false, true].map(move |ordinal| {
                        let display = if short {
                            format!(".{mode} {operand}")
                        } else {
                            format!(".Bd -{mode} -compact\n{operand}\n.Ed")
                        };
                        let source = if ordinal {
                            format!(
                                "{HEADER}.Bl -tag -width 6n\n.It 1.\n{display}\n.It 2.\n{display}\n.El\n{FOOTER}"
                            )
                        } else {
                            format!("{HEADER}BEFORE\n.br\n{display}\n.br\nAFTER\n{FOOTER}")
                        };
                        DisplayCase {
                            case: Case {
                                source,
                                name: format!("{mode}-{kind}-{ordinal}"),
                                source_line: 8,
                                disposition: if text.is_empty() && !empty_row { Retained } else { Recovered },
                            },
                            ordinal,
                            literal_count: usize::from(literal && !text.is_empty())
                                * (1 + usize::from(ordinal)),
                            text,
                            empty_row,
                        }
                    })
                })
        })
}

pub(crate) fn heads() -> impl Iterator<Item = Case> {
    // roff_term.c's pre-handlers consume the operands of these requests.
    // br/sp/ti/ce/rj keep their execution boundary even with a zero count.
    // mdoc_term.c consumes or skips Es, Db, Sm, and Tg's operands.
    [
        (".ft R", Recovered),
        (".ta 7n", Recovered),
        (".ll 72n", Recovered),
        (".po 0n", Recovered),
        (".ti 0n", Retained),
        (".sp 0", Retained),
        (".br", Retained),
        (".ce 0", Retained),
        (".rj 0", Retained),
        (".Es LEFT RIGHT", Recovered),
        (".Db GHOST", Recovered),
        (".Sm on", Recovered),
        (".Tg key", Recovered),
    ]
    .into_iter()
    .flat_map(|(control, disposition)| {
        [true, false].into_iter().flat_map(move |first| {
            [1, 2, 3].map(move |mask| {
                let mut source = format!("{HEADER}.Bl -tag -width 5n\n");
                for index in [1, 2] {
                    let control = if control.starts_with(".Tg ") {
                        format!("{control}{index}")
                    } else {
                        control.to_owned()
                    };
                    source.push_str(".It Xo\n");
                    if first && mask & index != 0 {
                        source.push_str(&format!("{control}\n"));
                    }
                    source.push_str(&format!("{index}.\n"));
                    if !first && mask & index != 0 {
                        source.push_str(&format!("{control}\n"));
                    }
                    source.push_str(".Xc\nBODY\n");
                }
                source.push_str(".El\n");
                source.push_str(FOOTER);
                Case {
                    name: format!("{control}-{first}-{mask}"),
                    source,
                    source_line: 8,
                    disposition,
                }
            })
        })
    })
}
