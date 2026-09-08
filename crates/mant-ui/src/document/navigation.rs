//! Sidebar projection of the source-neutral semantic index.
use super::{DocumentBuilder, NavKind, NavNode, SemanticEntry};
impl DocumentBuilder<'_> {
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
            let full_title = mant_protocol::entry_label(
                mant_protocol::EntryLabelMode::Forms,
                &entry.id,
                &entry.names,
                &entry.forms,
            );
            let title = mant_protocol::entry_label(
                mant_protocol::EntryLabelMode::Compact,
                &entry.id,
                &entry.names,
                &entry.forms,
            );
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

#[cfg(test)]
mod tests {
    #[test]
    fn sidebar_modes_use_shared_names_and_complete_form_fallbacks() {
        let query = mant_engine::query_roff_bytes(b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B -x, --language=LANG\nSelect language.\n.SH TERMS\n.TP\n.B find-new <subvolume> <last_gen>\nFind new files.\n").unwrap();
        let view = crate::DocumentView::new(&query);
        let nodes = view.navigation();
        let option = nodes.iter().find(|n| n.title == "-x, --language").unwrap();
        assert!(option.full_title.as_ref().unwrap().contains("LANG"));
        let term = nodes
            .iter()
            .find(|n| n.title == "find-new <subvolume> <last_gen>")
            .unwrap();
        assert!(term.full_title.is_none());
    }
}
