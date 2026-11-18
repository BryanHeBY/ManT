//! Native link occurrences and the destinations their operands prove.

use std::collections::BTreeMap;

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{LinkTarget, email_address_from_mailto_uri};

use super::{AstStructure, native::node_part_children};

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
}

/// The owned AST visits a link once at its canonical structural view. Equal
/// addresses and equal source coordinates can each belong to separate links.
pub(super) fn native_link_obligations(node: &Node, profile: &mut AstStructure) {
    if node.line > 0 && !node.flags.generated && !node.flags.no_print {
        match (node.kind, node.macro_token.as_deref()) {
            (NodeKind::Element, Some("Xr" | "MR")) => manual_link(node, profile),
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
            _ => {}
        }
    }
    for child in &node.children {
        native_link_obligations(child, profile);
    }
}

fn manual_link(node: &Node, profile: &mut AstStructure) {
    // CVS mdoc_xr_pre()/man_MR_pre() treat name and section separately;
    // optional MR suffix punctuation does not participate in the target.
    let Some(name) = node
        .children
        .first()
        .and_then(|operand| operand.text.as_deref())
    else {
        return;
    };
    if name.is_empty() {
        return;
    }
    profile.manual_links += 1;
    let section = node
        .children
        .get(1)
        .and_then(|operand| operand.text.as_deref());
    if let Some(name) = observe_operand(name)
        && let Some(section) = section.map_or(Some(None), |value| observe_operand(value).map(Some))
    {
        profile.known_links.record(Identity::Manual(name, section));
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
