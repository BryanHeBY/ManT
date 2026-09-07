//! Sidebar projection of the source-neutral semantic index.
use super::{DocumentBuilder, NavKind, NavNode, SemanticEntry};
impl DocumentBuilder {
    pub(super) fn entry_group(
        &mut self,
        owner_id: &str,
        target_id: &str,
        entries: &[SemanticEntry],
        depth: usize,
        is_last: bool,
    ) {
        if entries.is_empty() {
            return;
        }
        let summary = mant_ir::EntrySummary::for_entries(entries);
        let group_id = format!("__mant-entries__{owner_id}");
        let full_title = format!(
            "ENTRIES ({} direct · {} nested · {} {})",
            summary.direct,
            summary.descendants,
            summary.forms,
            if summary.forms == 1 { "form" } else { "forms" }
        );
        self.navigation(NavNode {
            id: group_id.clone(),
            target_id: target_id.to_owned(),
            title: format!("ENTRIES · {}", summary.direct),
            full_title: Some(full_title),
            depth,
            kind: NavKind::EntryGroup,
            has_children: true,
            is_last,
            parent_id: Some(owner_id.to_owned()),
        });
        self.semantic_entries(entries, depth + 1, &group_id);
    }

    pub(super) fn semantic_entries(
        &mut self,
        entries: &[SemanticEntry],
        depth: usize,
        parent_id: &str,
    ) {
        for (index, entry) in entries.iter().enumerate() {
            let full_title = (!entry.forms.is_empty())
                .then(|| entry.forms.join(" | "))
                .or_else(|| (!entry.aliases.is_empty()).then(|| entry.aliases.join(" | ")))
                .unwrap_or_else(|| entry.id.to_string());
            let title = (!entry.aliases.is_empty())
                .then(|| entry.aliases.join(" | "))
                .or_else(|| entry.forms.first().cloned())
                .unwrap_or_else(|| entry.id.to_string());
            self.navigation(NavNode {
                id: entry.id.to_string(),
                target_id: entry.id.to_string(),
                full_title: (full_title != title).then_some(full_title),
                title,
                depth,
                kind: NavKind::Entry(entry.kind),
                has_children: !entry.children.is_empty(),
                is_last: index + 1 == entries.len(),
                parent_id: Some(parent_id.to_owned()),
            });
            self.semantic_entries(&entry.children, depth + 1, &entry.id);
        }
    }
}
