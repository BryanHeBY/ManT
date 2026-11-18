//! Native link occurrences and the destinations their operands prove.

use std::collections::{BTreeMap, BTreeSet};

use libmandoc_rs::{Node, NodeKind, NormalizedListKind};
use mant_ir::{LinkTarget, email_address_from_mailto_uri};

use super::{AstStructure, native::node_part_children};

#[path = "links/sections.rs"]
mod sections;
pub(super) use sections::{NativeSectionLinks, ObservedSectionLinks};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Manual(String, Option<String>),
    External(String),
    Email(String),
}

#[derive(Default)]
pub(super) struct LinkOccurrences(BTreeMap<Identity, usize>);

impl LinkOccurrences {
    fn record(&mut self, identity: Identity) {
        *self.0.entry(identity).or_default() += 1;
    }

    pub(super) fn record_target(&mut self, target: &LinkTarget) {
        let identity = match target {
            LinkTarget::Manual {
                name,
                manual_section,
            } => Identity::Manual(name.clone(), manual_section.clone()),
            LinkTarget::External { uri } => Identity::External(uri.clone()),
            LinkTarget::Email { address } => Identity::Email(address.clone()),
            LinkTarget::Section { .. } | LinkTarget::Document { .. } => return,
        };
        self.record(identity);
    }

    pub(super) fn match_counts(&self, observed: &Self) -> (usize, usize) {
        let mut expected = 0;
        let mut matched = 0;
        for (identity, count) in &self.0 {
            expected += count;
            matched += count.min(observed.0.get(identity).unwrap_or(&0));
        }
        (expected, matched)
    }

    pub(super) fn len(&self) -> usize {
        self.0.values().sum()
    }
}

/// The owned AST visits a link once at its canonical structural view. Equal
/// addresses and equal source coordinates can each belong to separate links.
pub(super) fn native_link_obligations(
    report: &libmandoc_rs::ParseReport,
    profile: &mut AstStructure,
) {
    let error_origins = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.level == libmandoc_rs::DiagnosticLevel::Error)
        .filter_map(|diagnostic| diagnostic.location.map(|at| (at.line, at.column)))
        .collect();
    collect_links(&report.document.root, None, &error_origins, profile);
}

fn collect_links(
    node: &Node,
    list: Option<NormalizedListKind>,
    error_origins: &BTreeSet<(u32, u32)>,
    profile: &mut AstStructure,
) {
    let token = node.macro_token.as_deref();
    profile.section_outcomes.record_owner(node);
    if node.line > 0 && !node.flags.generated && !node.flags.no_print {
        match (node.kind, token) {
            (NodeKind::Element, Some(name @ ("Xr" | "MR"))) => {
                manual_link(node, name == "MR", profile);
            }
            // CVS man_UR_pre() reads the first operand of the HEAD and
            // emits a mailto template for MT; BODY contributes only text.
            (NodeKind::Block, Some(name @ ("UR" | "MT"))) => {
                if let Some(first) = node_part_children(node, NodeKind::Head).first() {
                    address_link(first, name == "MT", profile);
                }
            }
            // CVS mdoc_lk_pre() and mdoc__x_pre() read only the first
            // operand. The Lk label and its punctuation are not addresses.
            (NodeKind::Element, Some("Lk" | "%U")) => {
                if let Some(first) = node.children.first() {
                    address_link(first, false, profile);
                }
            }
            // CVS mdoc_mt_pre() emits a distinct href for each operand.
            (NodeKind::Element, Some("Mt")) => {
                for child in &node.children {
                    if !child.flags.delimiter_close && !child.flags.delimiter_open {
                        address_link(child, true, profile);
                    }
                }
            }
            (NodeKind::Element, Some("%R")) => rfc_link(node, profile),
            (NodeKind::Element, Some("Sx")) => {
                profile.section_links += usize::from(
                    profile
                        .section_outcomes
                        .record_reference(node, error_origins),
                );
            }
            (NodeKind::Block, Some("Sh" | "Ss")) => {
                profile.section_outcomes.record_heading(node);
            }
            _ => {}
        }
    }
    let list = if node.kind == NodeKind::Block && token == Some("Bl") {
        node.list_kind
    } else {
        list
    };
    // CVS mdoc_it_pre() skips the authored HEAD of marker and plain items.
    // The raw parser diagnostics and historical source-origin view still
    // describe that source; it cannot establish an executed link obligation.
    let skip_head = node.kind == NodeKind::Block
        && token == Some("It")
        && matches!(
            list,
            Some(
                NormalizedListKind::Bullet
                    | NormalizedListKind::Dash
                    | NormalizedListKind::Plain
                    | NormalizedListKind::Ordered
            )
        );
    for child in &node.children {
        if !skip_head || child.kind != NodeKind::Head {
            collect_links(child, list, error_origins, profile);
        }
    }
}

fn manual_link(node: &Node, man: bool, profile: &mut AstStructure) {
    // CVS mdoc_xr_pre()/man_MR_pre() treat name and section separately;
    // optional MR suffix punctuation does not participate in the target.
    let Some(name) = node
        .children
        .first()
        .and_then(|operand| operand.text.as_deref())
    else {
        return;
    };
    let section = node
        .children
        .get(1)
        .and_then(|operand| operand.text.as_deref());
    let (Some(name), Some(manual_section)) = (
        observe_operand(name),
        section.map_or(Some(None), |value| observe_operand(value).map(Some)),
    ) else {
        profile.unclassified_manual_links += 1;
        return;
    };
    if name.trim().is_empty() && manual_section.is_none() && !man {
        // A sectionless mdoc Xr has no generated parentheses. A name with
        // no nonwhitespace glyph then produces at most inert spacing.
        return;
    }
    let target = LinkTarget::Manual {
        name,
        manual_section,
    };
    if target.to_uri().is_some() {
        profile.manual_links += 1;
        profile.known_links.record_target(&target);
    } else {
        profile.invalid_manual_links.record_target(&target);
    }
}

fn address_link(operand: &Node, email_only: bool, profile: &mut AstStructure) {
    let Some(source) = operand.text.as_deref().filter(|_| !operand.flags.no_print) else {
        return;
    };
    let Some(address) = observe_operand(source) else {
        if email_only {
            profile.email_links += 1;
        } else {
            profile.unclassified_address_links += 1;
        }
        return;
    };
    if address.is_empty() {
        // A proven empty decoding cannot yield an addressable target.
        return;
    }
    if email_only {
        profile.email_links += 1;
        profile.known_links.record(Identity::Email(address));
    } else if let Some(email) = email_address_from_mailto_uri(&address) {
        profile.email_links += 1;
        profile.known_links.record(Identity::Email(email));
    } else {
        profile.external_links += 1;
        profile.known_links.record(Identity::External(address));
    }
}

fn rfc_link(node: &Node, profile: &mut AstStructure) {
    // CVS mdoc__x_pre() checks the raw first operand before href decoding.
    let Some(number) = node
        .children
        .first()
        .and_then(|operand| operand.text.as_deref())
        .and_then(|argument| argument.strip_prefix("RFC "))
    else {
        return;
    };
    if number.bytes().all(|byte| byte.is_ascii_digit()) {
        profile.external_links += 1;
        profile.known_links.record(Identity::External(format!(
            "https://www.rfc-editor.org/rfc/rfc{number}.html"
        )));
    }
}

fn observe_operand(source: &str) -> Option<String> {
    if source
        .bytes()
        .all(|byte| byte.is_ascii_graphic() && byte != b'\\')
    {
        Some(source.to_owned())
    } else {
        mant_codec::observe_roff_link_operand(source)
    }
}
