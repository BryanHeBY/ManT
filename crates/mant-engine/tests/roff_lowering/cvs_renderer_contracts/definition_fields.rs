use super::*;

/// A separate authored section bounds the body independently of OS metadata.
/// `mdoc_validate.c::post_os` accepts arbitrary text, and `print_mdoc_foot()`
/// may lay out that text across several rows.
fn framed_definition_rows(output: &str) -> Vec<String> {
    let lines = output.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.trim() == "DESCRIPTION")
        .expect("authored DESCRIPTION heading");
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim() == "NEXT")
        .map(|index| index + start + 1)
        .expect("authored NEXT heading before page furniture");
    let mut rows = lines[start + 1..end]
        .iter()
        .map(|line| line.trim().to_owned())
        .collect::<Vec<_>>();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

#[path = "definition_fields/accepted_fields.rs"]
mod accepted_fields;
#[path = "definition_fields/authored_identity.rs"]
mod authored_identity;
#[path = "definition_fields/device_rows.rs"]
mod device_rows;
#[path = "definition_fields/field_controls.rs"]
mod field_controls;
#[path = "definition_fields/fonts.rs"]
mod fonts;
#[path = "definition_fields/hang_layout.rs"]
mod hang_layout;
#[path = "definition_fields/row_ownership.rs"]
mod row_ownership;
#[path = "definition_fields/run_in_fields.rs"]
mod run_in_fields;
