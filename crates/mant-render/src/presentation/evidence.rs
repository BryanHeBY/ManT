//! Fixed evidence accounting labels shared by all presentations.
use mant_protocol::EvidenceClass;
use std::fmt::Write;

/// Render every category count, including zero totals and off-page definitions.
#[must_use]
pub fn render_evidence_counts(counts: &mant_protocol::EvidenceCounts) -> String {
    let mut text = String::new();
    for class in EvidenceClass::ALL {
        let count = counts.get(class);
        write!(
            text,
            "\n{}: total={}, returned={}",
            class.title(),
            count.total,
            count.returned
        )
        .expect("String writer");
    }
    if counts.direct_entry.total == 0 {
        text.push_str("\nNo direct entry collected; any following records are original mentions or declared relationships, not proof that an option is absent.");
    } else if counts.direct_entry.returned == 0 {
        text.push_str("\nDirect entries exist but are not included on this page.");
    }
    text
}
