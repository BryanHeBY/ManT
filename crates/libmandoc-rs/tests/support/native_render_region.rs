//! Native scope selection projects only heading overstrikes, retaining body bytes.

pub fn description_body(raw: &str) -> &str {
    let mut start = None;
    let mut offset = 0;
    for row in raw.split_inclusive('\n') {
        let mut projected = String::new();
        for scalar in row.chars() {
            if scalar == '\u{8}' {
                projected.pop();
            } else {
                projected.push(scalar);
            }
        }
        if projected == "DESCRIPTION\n" {
            start = Some(offset + row.len());
        } else if projected == "NEXT\n" {
            return &raw[start.unwrap()..offset];
        }
        offset += row.len();
    }
    panic!("missing native DESCRIPTION/NEXT scope")
}
