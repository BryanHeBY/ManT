//! Live product checks for the six main acceptance examples.
//!
//! Every case runs through three product projections: plain rows drive the
//! row axes, an ANSI-styled projection must not move the layout, and the
//! JSON contract round-trips without private markers or row changes. The
//! registered axes are then driven by [`super::assert_registered_axes`]:
//! `must` axes hold today, `after_repair` axes are divergences the
//! comparator has to keep detecting until their repair unit flips them.

use super::acceptance_cases::{MAIN_EXAMPLES, case_by_name};
use super::axis_model::{AxisKind, Observed, Owner, UnitStyle, row_words};
use super::{assert_registered_axes, evaluate_case, load_case, validate_against_oracle};
use mant_ir::{
    Block, Document, Inline, LinkTarget, ResolvedContent, Section,
    visit::{self, Visit},
};

/// Apply the device cell projection: a backspace pops the previous cell,
/// nonbreaking blanks keep their spelling.
fn project_device(output: &str) -> String {
    let mut projected = String::with_capacity(output.len());
    for character in output.chars() {
        if character == '\u{8}' {
            projected.pop();
        } else {
            projected.push(character);
        }
    }
    projected
}

/// Rows strictly between the structural DESCRIPTION and NEXT heading rows.
/// Nothing inside the window is trimmed: interior and trailing blank rows
/// and the common margin stay exactly as rendered.
fn region_rows(output: &str) -> Vec<String> {
    let rows: Vec<String> = project_device(output).lines().map(str::to_owned).collect();
    let window = |heading: &str| -> Vec<usize> {
        rows.iter()
            .enumerate()
            .filter(|(_, row)| row.as_str() == heading)
            .map(|(index, _)| index)
            .collect()
    };
    let starts = window("DESCRIPTION");
    let ends = window("NEXT");
    assert_eq!(
        starts.len(),
        1,
        "expected exactly one DESCRIPTION heading row: {starts:?}"
    );
    assert_eq!(
        ends.len(),
        1,
        "expected exactly one NEXT heading row: {ends:?}"
    );
    rows[starts[0] + 1..ends[0]].to_vec()
}

fn description_section(document: &Document) -> &Section {
    document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap_or_else(|| panic!("no DESCRIPTION section in the lowered document"))
}

fn block_source_line(block: &Block) -> Option<u32> {
    let source = match block {
        Block::Paragraph { source, .. }
        | Block::Preformatted { source, .. }
        | Block::List { source, .. }
        | Block::DefinitionList { source, .. }
        | Block::Table { source, .. }
        | Block::Equation { source, .. }
        | Block::VerticalSpace { source, .. }
        | Block::ThematicBreak { source, .. }
        | Block::Unsupported { source, .. } => source,
    };
    source.as_ref().map(|span| span.line)
}

/// Inline facts of the DESCRIPTION section, in document order: typed
/// identity occurrences with their visible labels, each observed word with
/// its owning visible range and style, and the source line of the block
/// that carries it (unit-level authored lines arrive with the ownership
/// repair; until then the block line is what the model provides).
#[derive(Default)]
struct InlineFacts {
    identities: Vec<(String, String)>,
    word_owners: Vec<(String, Owner)>,
    word_styles: Vec<(String, UnitStyle)>,
    word_sources: Vec<(String, u32)>,
    link_depth: usize,
    identity_index: usize,
    style_stack: Vec<UnitStyle>,
    block_lines: Vec<Option<u32>>,
}

impl InlineFacts {
    fn current_line(&self) -> Option<u32> {
        self.block_lines.iter().rev().find_map(|line| *line)
    }

    fn record(&mut self, value: &str, style: UnitStyle) {
        let owner = if self.link_depth > 0 {
            Owner::Link(self.identity_index)
        } else {
            Owner::None
        };
        let line = self.current_line();
        for word in row_words(value) {
            let word = word.to_owned();
            self.word_owners.push((word.clone(), owner));
            self.word_styles.push((word.clone(), style));
            if let Some(line) = line {
                self.word_sources.push((word, line));
            }
        }
    }
}

impl<'ir> Visit<'ir> for InlineFacts {
    fn visit_block(&mut self, block: &'ir Block) {
        self.block_lines.push(block_source_line(block));
        visit::walk_block(self, block);
        self.block_lines.pop();
    }

    fn visit_inline(&mut self, inline: &'ir Inline) {
        match inline {
            Inline::Link {
                target, children, ..
            } => {
                if let LinkTarget::External { uri } = target {
                    self.identities
                        .push((uri.clone(), mant_ir::inline_plain_text(children)));
                    self.identity_index = self.identities.len() - 1;
                }
                self.link_depth += 1;
                visit::walk_inline(self, inline);
                self.link_depth -= 1;
            }
            Inline::Text { value } => {
                let style = self.style_stack.last().copied().unwrap_or(UnitStyle::Plain);
                self.record(value, style);
            }
            Inline::Code { value } => self.record(value, UnitStyle::Code),
            Inline::Strong { .. } => {
                self.style_stack.push(UnitStyle::Strong);
                visit::walk_inline(self, inline);
                self.style_stack.pop();
            }
            Inline::Emphasis { .. } => {
                self.style_stack.push(UnitStyle::Emphasis);
                visit::walk_inline(self, inline);
                self.style_stack.pop();
            }
            _ => visit::walk_inline(self, inline),
        }
    }
}

/// Observe one lowering through its plain rows and its semantic model.
fn observe(query: &ResolvedContent) -> Observed {
    let document = query
        .document
        .as_ref()
        .expect("roff lowering resolves a document");
    let mut facts = InlineFacts::default();
    visit::walk_section(&mut facts, description_section(document));
    let plain = mant_render::render_query_man(query);
    Observed {
        rows: region_rows(&plain),
        identities: facts.identities,
        ownership: facts.word_owners,
        scalar_ranges: Vec::new(),
        styles: facts.word_styles,
        sources: facts.word_sources,
    }
}

/// The committed case set stays pinned to the recorded eleven cases.
#[test]
fn acceptance_case_set_is_pinned() {
    let names = super::case_names();
    for name in MAIN_EXAMPLES
        .into_iter()
        .chain(super::acceptance_cases::AUXILIARY_EXAMPLES)
    {
        assert!(
            names.iter().any(|committed| committed == name),
            "{name}: case file missing from the committed set"
        );
    }
}

/// Declared expectations match the recorded oracle windows, so a
/// re-recorded snapshot cannot silently drift away from any card.
#[test]
fn declared_expectations_match_the_recorded_oracle_rows() {
    for name in MAIN_EXAMPLES
        .into_iter()
        .chain(super::acceptance_cases::AUXILIARY_EXAMPLES)
    {
        let case = case_by_name(name);
        let files = load_case(name);
        validate_against_oracle(&case, &files);
    }
}

/// The six main examples across plain, ANSI and JSON projections.
#[test]
fn six_examples_assert_registered_axes_across_projections() {
    for name in MAIN_EXAMPLES {
        let case = case_by_name(name);
        let files = load_case(name);
        let query = mant_loader::load_roff_bytes(files.source.as_bytes())
            .unwrap_or_else(|error| panic!("{}: lower case: {error}", case.id));

        // Projection 2: legal ANSI decoration must not move the layout.
        let plain = mant_render::render_query_man(&query);
        let styled = mant_render::render_query_text_with(&query, |_, text| {
            format!("\u{1b}[1m{text}\u{1b}[0m")
        });
        let stripped = styled.replace("\u{1b}[1m", "").replace("\u{1b}[0m", "");
        assert_eq!(
            stripped, plain,
            "{}: legal ANSI decoration changed the layout",
            case.id
        );

        // Projection 3: the JSON contract round-trips without private
        // markers and without moving a single region row.
        let json = mant_render::render_query_json(&query, false)
            .unwrap_or_else(|error| panic!("{}: render json: {error}", case.id));
        assert!(
            !json.contains("\\u0000mant:"),
            "{}: private execution owner escaped the JSON contract",
            case.id
        );
        let bundle: mant_protocol::QueryBundle = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("{}: decode bundle: {error}", case.id));
        let round_tripped = mant_render::render_query_man(&bundle.into());
        assert_eq!(
            region_rows(&round_tripped),
            region_rows(&plain),
            "{}: the JSON round-trip moved region rows",
            case.id
        );

        // Projection 1 drives the registered axes.
        let observed = observe(&query);
        let report = evaluate_case(&case, &files, &observed);
        assert_registered_axes(&case, &report);
    }
}

/// The acceptance base has to catch the review regressions it was built
/// for: the tag explicit vertical spacing and the hang final-word gap.
/// This is the detection proof for the registered criteria; when a repair
/// closes one of them, its expectation flips from `after_repair` to
/// `must` in the same commit, and its proof entry retires with it (the
/// column tail hard row flipped with its repair).
#[test]
fn comparator_detects_the_registered_hard_row_blank_and_separator_regressions() {
    let proofs: [(&str, AxisKind); 2] = [
        ("tag_explicit_vspace", AxisKind::BlankCount),
        ("hang_final_gap", AxisKind::Separator),
    ];
    for (name, kind) in proofs {
        let case = case_by_name(name);
        let files = load_case(name);
        let query = mant_loader::load_roff_bytes(files.source.as_bytes())
            .unwrap_or_else(|error| panic!("{}: lower case: {error}", case.id));
        let observed = observe(&query);
        let report = evaluate_case(&case, &files, &observed);
        assert!(
            report.fails_on(kind),
            "{}: the {} axis no longer detects the registered regression; \
             the repair landed, flip its registration to must",
            case.id,
            kind.name()
        );
    }
}
