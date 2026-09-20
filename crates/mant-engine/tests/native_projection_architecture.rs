//! End-to-end contracts for the K23 native execution projection.
//!
//! Each source below was run through the pinned CVS reference binary before
//! these assertions were written.  The reference binary SHA-256 is
//! `3468a220866e1b3ea77bd0d25775700b5066762471952be9891dcec0482e93c6`.

use mant_ir::{Block, visit::Visit};
use mant_loader::load_roff_bytes;
use mant_render::render_query_text;

fn rendered(source: &str) -> String {
    let content = load_roff_bytes(source.as_bytes()).expect("load native roff fixture");
    assert!(mant_ir::validate_document(content.document.as_ref().expect("document")).is_empty());
    render_query_text(&content)
}

fn row<'a>(text: &'a str, token: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(token))
        .unwrap_or_else(|| panic!("missing row containing {token:?}: {text:?}"))
}

fn exact_row<'a>(text: &'a str, token: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim() == token)
        .unwrap_or_else(|| panic!("missing exact row {token:?}: {text:?}"))
}

fn leading_columns(line: &str) -> usize {
    line.chars().take_while(|value| *value == ' ').count()
}

fn blank_rows_between(text: &str, before: &str, after: &str) -> usize {
    let rows = text.lines().collect::<Vec<_>>();
    let before = rows.iter().position(|line| line.trim() == before).unwrap();
    let after = rows.iter().position(|line| line.trim() == after).unwrap();
    assert!(
        rows[before + 1..after]
            .iter()
            .all(|line| line.trim().is_empty()),
        "{text:?}"
    );
    after - before - 1
}

#[test]
fn nested_structures_have_one_material_owner() {
    // Pinned CVS `print_mdoc_node()` executes the Bd wrapper around its
    // children, while `term_tbl()` exclusively renders the table child.  The
    // parent display does not replay the child's atom envelope.
    let source = ".Dd September 20, 2026\n.Dt PROBE 7\n.Os\n.Sh DESCRIPTION\n.Bd -literal\nBEFORE\n.TS\nl l.\nWORD\tNEXT\n.TE\nAFTER\n.Ed\n";
    let text = rendered(source);
    for token in ["BEFORE", "WORD", "NEXT", "AFTER"] {
        assert_eq!(text.matches(token).count(), 1, "{text:?}");
    }
    let positions = ["BEFORE", "WORD", "AFTER"].map(|token| text.find(token).unwrap());
    assert!(positions[0] < positions[1] && positions[1] < positions[2]);
}

#[test]
fn causal_boundaries_are_projected_once() {
    // Fixed CVS `man_term.c::print_bvspace()` calls `term_newln()` and then
    // `term_vspace()` twice; `term_vspace()` owns the two blank device rows.
    let paragraph = rendered(
        ".TH PROBE 1 \"September 20, 2026\"\n.SH DESCRIPTION\nBEFORE\n.PD 2\n.PP\nAFTER\n",
    );
    assert_eq!(blank_rows_between(&paragraph, "BEFORE", "AFTER"), 2);

    // A source blank row in the EX no-fill region is one hard row boundary,
    // not a boundary plus a second device-line reconstruction.
    let example = rendered(
        ".TH PROBE 1 \"September 20, 2026\"\n.SH DESCRIPTION\n.EX\nFIRST\n\nSECOND\n.EE\n",
    );
    assert_eq!(blank_rows_between(&example, "FIRST", "SECOND"), 1);
}

#[test]
fn native_origins_are_relative_to_the_parent_frame() {
    // Pinned CVS `termp_it_pre()` applies `-offset 3n` before establishing the
    // tag field.  The source-neutral reader removes the common page origin,
    // so the tag retains a three-column parent-relative indent.
    let list = rendered(
        ".Dd September 20, 2026\n.Dt PROBE 7\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width 12n -offset 3n\n.It TAG\nBODY\n.El\n",
    );
    assert_eq!(leading_columns(row(&list, "TAG")), 3, "{list:?}");

    // Fixed CVS `man_term.c::pre_in()` persists the changed offset across the
    // explicit break; B and C therefore have the same origin, three columns
    // beyond A in this deterministic terminal profile.
    let input =
        rendered(".TH PROBE 1 \"September 20, 2026\"\n.SH DESCRIPTION\nA\n.in 8n\nB\n.br\nC\n");
    let a = leading_columns(exact_row(&input, "A"));
    let b = leading_columns(exact_row(&input, "B"));
    let c = leading_columns(exact_row(&input, "C"));
    assert_eq!(b, c, "{input:?}");
    assert_eq!(b - a, 3, "{input:?}");
}

#[test]
fn table_layout_rules_suppress_data_without_breaking_the_document() {
    // Fixed CVS `tbl_data()` gives the layout rule precedence over the
    // associated tbl_dat payload.  `term_tbl()` renders one rule and never
    // renders `IGNORED`; parser data existence and renderer invocation are
    // deliberately distinct facts.
    let source = ".TH PROBE 1 \"September 20, 2026\"\n.SH DESCRIPTION\n.TS\n_.\nIGNORED\n.TE\n";
    let content = load_roff_bytes(source.as_bytes()).expect("layout rule table must load");
    let text = render_query_text(&content);
    assert!(!text.contains("IGNORED"), "{text:?}");

    struct Tables(usize);
    impl<'a> Visit<'a> for Tables {
        fn visit_block(&mut self, block: &'a Block) {
            if matches!(block, Block::Table { .. }) {
                self.0 += 1;
            }
            mant_ir::visit::walk_block(self, block);
        }
    }
    let mut tables = Tables(0);
    tables.visit_document(content.document.as_ref().unwrap());
    assert_eq!(tables.0, 1);
}

#[test]
fn table_source_equation_keeps_the_cvs_execution_topology() {
    // The pinned CVS reference and `-Ttree` both show an empty tbl cell first,
    // followed by the EQN as a sibling in the SH body.  `man_term.c` dispatches
    // ROFFT_TBL and ROFFT_EQN independently; `tbl_term.c` only brackets the
    // tbl_data invocation.  Projection must preserve that execution topology
    // and materialize each specialized producer exactly once.
    let source = ".TH K23-TABLE-EQN 1 \"September 20, 2026\" \"ManT\" \"Manual\"\n.SH DESCRIPTION\n.TS\nl.\nT{\n.EQ\nx sup 2\n.EN\nT}\n.TE\n";
    let content = load_roff_bytes(source.as_bytes()).expect("nested table equation must load");
    let text = render_query_text(&content);
    assert_eq!(text.matches('x').count(), 1, "{text:?}");
    assert!(text.contains('2'), "{text:?}");

    struct Structure {
        tables: usize,
        equations: usize,
        equations_in_table_cells: usize,
    }
    impl<'a> Visit<'a> for Structure {
        fn visit_block(&mut self, block: &'a Block) {
            match block {
                Block::Table { rows, .. } => {
                    self.tables += 1;
                    self.equations_in_table_cells += rows
                        .iter()
                        .flat_map(|row| &row.cells)
                        .flat_map(|cell| &cell.blocks)
                        .filter(|child| matches!(child, Block::Equation { .. }))
                        .count();
                }
                Block::Equation { .. } => self.equations += 1,
                _ => {}
            }
            mant_ir::visit::walk_block(self, block);
        }
    }
    let mut structure = Structure {
        tables: 0,
        equations: 0,
        equations_in_table_cells: 0,
    };
    structure.visit_document(content.document.as_ref().unwrap());
    assert_eq!(structure.tables, 1);
    assert_eq!(structure.equations, 1);
    assert_eq!(structure.equations_in_table_cells, 0);
}
