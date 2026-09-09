//! Borrowed source facts adapted to the transport-neutral label policy.
use mant_ir::EntryOwner;
use mant_protocol::{EntryLabelMode, entry_label};

pub(crate) fn owner_label(owner: EntryOwner<'_>, names: &[String], mode: EntryLabelMode) -> String {
    let id = owner.facts().expect("indexed entry label").id.as_str();
    // Avoid projecting styled forms for the common names-only label path.
    if mode == EntryLabelMode::Compact && !names.is_empty() {
        return entry_label(mode, id, names, std::iter::empty::<&str>());
    }
    let forms = owner.forms().unwrap_or_default();
    entry_label(
        mode,
        id,
        names,
        forms.iter().map(mant_ir::inline_plain_text),
    )
}
