//! Borrowed source facts adapted to the transport-neutral label policy.
use mant_ir::{ContentContext, EntryOwner};
use mant_protocol::{EntryLabelMode, entry_label};

pub(crate) fn owner_label(
    content: ContentContext<'_>,
    owner: EntryOwner<'_>,
    names: &[String],
    mode: EntryLabelMode,
) -> String {
    let id = owner.facts().expect("indexed entry label").id.as_str();
    // Avoid projecting styled forms for the common names-only label path.
    if mode == EntryLabelMode::Compact && !names.is_empty() {
        return entry_label(mode, id, names, std::iter::empty::<&str>());
    }
    let forms = content
        .entry_forms(owner)
        .expect("document entry forms resolve in their own content store")
        .unwrap_or_default();
    entry_label(
        mode,
        id,
        names,
        forms.iter().map(|form| {
            content
                .plain_text(form)
                .expect("document entry forms resolve in their own content store")
        }),
    )
}
