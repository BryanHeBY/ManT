use super::*;

pub(super) struct Case {
    pub(super) name: String,
    pub(super) leaf: Leaf,
    pub(super) linked: bool,
    edge: Edge,
    hard_rows: usize,
    literal_tail: usize,
}

impl Case {
    pub(super) fn markdown(&self) -> String {
        let root = match self.leaf {
            Leaf::Fence => format!("```seam\n{BODY}{}\n```", "\n".repeat(self.literal_tail)),
            Leaf::Rule => "***".into(),
            _ => unreachable!("finite structural seeds"),
        };
        let link = if self.linked {
            format!("[]({URI})")
        } else {
            String::new()
        };
        let boundary = if self.edge == Edge::Leading {
            format!("{link}{}", "&#10;".repeat(self.hard_rows))
        } else {
            format!("{link}{}", "<br />".repeat(self.hard_rows))
        };
        let blocks = if self.edge == Edge::Leading {
            format!("{boundary}\n{root}")
        } else {
            format!("{root}\n{boundary}")
        };
        format!("# Probe\n\n{blocks}")
    }

    pub(super) fn expected(&self) -> String {
        // The reader maps each LF entity / standard br to content, and
        // removes only a fence's one framing LF. Independent reading Flow
        // closes an authored empty row at EOF, rather than inventing a gap.
        let word = if self.leaf == Leaf::Rule { "---" } else { BODY };
        let before = 2 + if self.edge == Edge::Leading {
            self.hard_rows
        } else {
            0
        };
        let after = self.literal_tail
            + if self.edge == Edge::Trailing {
                self.hard_rows + 1
            } else {
                usize::from(self.literal_tail > 0)
            };
        format!("Probe{}{word}{}", "\n".repeat(before), "\n".repeat(after))
    }
}

pub(super) fn cases() -> Vec<Case> {
    let mut cases = vec![];
    for leaf in [Leaf::Fence, Leaf::Rule] {
        for edge in [Edge::Leading, Edge::Trailing] {
            for hard_rows in [1, 2] {
                for linked in [false, true] {
                    for literal_tail in 0..=usize::from(leaf == Leaf::Fence) {
                        cases.push(Case {
                            name: format!("{leaf:?}/{edge:?}/{hard_rows}/{linked}/{literal_tail}"),
                            leaf,
                            linked,
                            edge,
                            hard_rows,
                            literal_tail,
                        });
                    }
                }
            }
        }
    }
    cases
}
