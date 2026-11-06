//! Native word fonts and typed reference owners, independently of UI themes.
use mant_ir::{Inline, visit::Visit};
#[derive(Clone, Copy, Default)]
struct Role {
    strong: bool,
    emphasis: bool,
    code: bool,
}
#[derive(Default)]
struct Glyphs {
    role: Role,
    cells: Vec<(char, Role)>,
    links: Vec<(String, mant_ir::LinkTarget)>,
}
impl<'a> Visit<'a> for Glyphs {
    fn visit_block(&mut self, block: &'a mant_ir::Block) {
        self.cells.push(('\n', self.role));
        mant_ir::visit::walk_block(self, block);
        self.cells.push(('\n', self.role));
    }
    fn visit_heading(&mut self, heading: &'a mant_ir::Heading) {
        self.cells.push(('\n', self.role));
        mant_ir::visit::walk_heading(self, heading);
        self.cells.push(('\n', self.role));
    }

    fn visit_inline(&mut self, inline: &'a Inline) {
        match inline {
            Inline::Text { value } => {
                self.cells.extend(value.chars().map(|c| (c, self.role)));
            }
            Inline::Code { value } => self.cells.extend(value.chars().map(|c| {
                (
                    c,
                    Role {
                        code: true,
                        ..self.role
                    },
                )
            })),
            Inline::LineBreak { .. } => self.cells.push(('\n', self.role)),
            Inline::Strong { children } => {
                let saved = self.role;
                self.role.strong = true;
                for child in children {
                    self.visit_inline(child);
                }
                self.role = saved;
            }
            Inline::Emphasis { children } => {
                let saved = self.role;
                self.role.emphasis = true;
                for child in children {
                    self.visit_inline(child);
                }
                self.role = saved;
            }
            Inline::Link {
                target, children, ..
            } => {
                self.links
                    .push((mant_ir::inline_plain_text(children), target.clone()));
                for child in children {
                    self.visit_inline(child);
                }
            }
            _ => mant_ir::visit::walk_inline(self, inline),
        }
    }
}
fn assert_role(glyphs: &Glyphs, word: &str, role: &str) {
    let text = glyphs.cells.iter().map(|(c, _)| c).collect::<String>();
    let positions = crate::cases::positions(&text, word);
    assert_eq!(positions.len(), 1, "{word}: {text}");
    let begin = text[..positions[0]].chars().count();
    for (_, actual) in &glyphs.cells[begin..begin + word.chars().count()] {
        assert_eq!(
            (actual.strong, actual.emphasis, actual.code),
            match role {
                "regular" => (false, false, false),
                "emphasis" => (false, true, false),
                "strong" | "bold" => (true, false, false),
                other => panic!("unregistered role {other}"),
            },
            "{word}"
        );
    }
}

#[test]
fn table_fonts_and_following_reference_owners_keep_their_native_registers() {
    let gold: serde_json::Value =
        serde_json::from_str(include_str!("../roff_lowering/table_execution/styles.json")).unwrap();
    let cases = crate::cases::cases();
    for expected in gold["cases"].as_array().unwrap() {
        let Some(case) = cases
            .iter()
            .find(|c| c.id == expected["id"].as_str().unwrap())
        else {
            continue;
        };
        let content = crate::json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        let mut glyphs = Glyphs::default();
        glyphs.visit_document(content.document.as_ref().unwrap());
        if let Some(role) = expected["label_role"].as_str() {
            assert_role(&glyphs, &case.expected_label, role);
            for word in ["BETA", "AFTER"] {
                assert_role(
                    &glyphs,
                    word,
                    expected[format!("{word}_role")].as_str().unwrap(),
                );
            }
        }
        if let Some(roles) = expected["following_roles"].as_object() {
            for (word, role) in roles {
                assert_role(&glyphs, word, role.as_str().unwrap());
            }
        }
        if case.id == "T3-font-register" {
            assert_role(&glyphs, "CELL", "bold");
            assert_role(&glyphs, "ALPHA", "emphasis");
            assert_role(&glyphs, "BETA", "bold");
            assert_role(&glyphs, "AFTER", "bold");
        }
        for field in ["enhanced_cell", "first_cell"] {
            if let Some(word) = expected[field]["value"].as_str() {
                let role = if expected[field]["bold"] == true {
                    "bold"
                } else if expected[field]["emphasis"] == true {
                    "emphasis"
                } else {
                    "regular"
                };
                assert_role(&glyphs, word, role);
            }
        }
        if let Some(label) = expected["second_cell_enhancement"]["label"].as_str() {
            assert_role(&glyphs, label, "emphasis");
            let target: mant_ir::LinkTarget =
                serde_json::from_value(expected["second_cell_enhancement"]["target"].clone())
                    .unwrap();
            assert_eq!(
                glyphs
                    .links
                    .iter()
                    .filter(|(l, t)| l == label && *t == target)
                    .count(),
                1,
                "accepted sibling Link owner"
            );
        }
        if !expected["label_target"].is_null() {
            let target: mant_ir::LinkTarget =
                serde_json::from_value(expected["label_target"].clone()).unwrap();
            assert_eq!(
                glyphs
                    .links
                    .iter()
                    .filter(|(label, t)| label == &case.expected_label && t == &target)
                    .count(),
                1,
                "{}",
                case.id
            );
        }
    }
}

pub(super) fn assert_following_links(
    content: &mant_ir::ResolvedContent,
    case: &crate::cases::Case,
    round: usize,
) {
    let mut glyphs = Glyphs::default();
    glyphs.visit_document(content.document.as_ref().unwrap());
    let target = mant_ir::LinkTarget::External {
        uri: "https://example.test".into(),
    };
    let expected = usize::from(
        case.source.contains("https://example.test")
            && !(round > 0 && case.id.starts_with("T2-mdoc-column")),
    );
    assert_eq!(
        glyphs
            .links
            .iter()
            .filter(|(label, t)| label == &case.expected_label && *t == target)
            .count(),
        expected,
        "{} round{round}: one complete typed following label; portable outer table fences flatten references",
        case.id
    );
}
