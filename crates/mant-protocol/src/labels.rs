//! Stable entry labels; fallback text never creates a selectable name.

/// Which already validated entry facts supply a display label first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryLabelMode {
    /// Names, then complete visible forms, then the existing identity.
    Compact,
    /// Complete visible forms, then the compact fallback.
    Forms,
}

/// Produce a nonempty entry label without parsing names or implying aliases.
///
/// Callers supply validated names and visible form text. This does not perform
/// terminal escaping: adapters sanitize the resulting label for their surface.
#[must_use]
pub fn entry_label<S: AsRef<str>>(
    mode: EntryLabelMode,
    id: &str,
    names: &[String],
    forms: impl IntoIterator<Item = S>,
) -> String {
    if mode == EntryLabelMode::Compact && !names.is_empty() {
        return names.join(", ");
    }
    let forms = forms
        .into_iter()
        .filter_map(|form| {
            let text = form.as_ref();
            (!text.trim().is_empty()).then(|| text.to_owned())
        })
        .collect::<Vec<_>>();
    if !forms.is_empty() {
        forms.join(" | ")
    } else if !names.is_empty() {
        names.join(", ")
    } else {
        id.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_modes_keep_names_forms_and_identity_distinct() {
        let names = ["-x".to_owned(), "--language".to_owned()];
        let forms = ["-x LANG", "--language=LANG"];
        assert_eq!(
            entry_label(EntryLabelMode::Compact, "id", &names, forms),
            "-x, --language"
        );
        assert_eq!(
            entry_label(EntryLabelMode::Forms, "id", &names, forms),
            "-x LANG | --language=LANG"
        );
        for mode in [EntryLabelMode::Compact, EntryLabelMode::Forms] {
            assert_eq!(
                entry_label(mode, "id", &[], ["find-new ARG"]),
                "find-new ARG"
            );
            assert_eq!(entry_label(mode, "id", &[], ["", " "]), "id");
            assert_eq!(entry_label(mode, "id", &names, [""]), "-x, --language");
        }
    }
}
