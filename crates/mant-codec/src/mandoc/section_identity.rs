//! Authored section names and validated native heading destinations.

use std::{borrow::Cow, collections::HashMap};

use libmandoc_rs::{Node, NodeKind};

use super::roff_escape::{RoffInlineEvent, decode};

/// Section identity from the AST, independent of formatter-generated labels.
#[derive(Clone)]
pub struct RoffSectionHeading {
    pub(super) title: String,
    native_tag: Option<String>,
}

impl RoffSectionHeading {
    /// Read the recursive heading phrase and the validated HTML HEAD tag.
    #[must_use]
    pub fn from_node(node: &Node) -> Self {
        let head = node
            .children
            .iter()
            .find(|child| child.kind == NodeKind::Head);
        let title = head.map_or_else(String::new, |head| authored_section_phrase(&head.children));
        let native_tag = head
            .filter(|head| head.flags.deep_link_target)
            .and_then(|head| {
                // print_otag_id() uses a validated tag first. html_make_id()
                // can derive it without a tag only for direct TEXT children.
                head.tag.as_deref().or_else(|| {
                    head.children
                        .iter()
                        .all(|child| child.kind == NodeKind::Text)
                        .then_some(title.as_str())
                })
            })
            .filter(|target| !target.is_empty())
            .map(str::to_owned);
        Self { title, native_tag }
    }
}

/// Map authored section names and surviving native targets to canonical IDs.
#[derive(Default)]
pub struct RoffSectionTargets {
    authored: HashMap<String, Option<String>>,
    exact_native: HashMap<String, Option<String>>,
    native_word_keys: HashMap<String, Option<String>>,
}

impl RoffSectionTargets {
    /// Associate one source heading with its canonical `ManT` ID.
    pub fn register(&mut self, heading: RoffSectionHeading, id: String) {
        if let Some(tag) = heading.native_tag {
            register_name(
                &mut self.native_word_keys,
                native_word_key(&tag).into_owned(),
                id.clone(),
            );
            register_name(&mut self.exact_native, tag, id.clone());
        }
        if !heading.title.trim().is_empty() {
            register_name(&mut self.authored, heading.title, id);
        }
    }

    /// Resolve the validated native heading target, the exact authored title,
    /// or `ManT`'s uniquely parenthetically qualified title, in that order.
    #[must_use]
    pub fn resolve(&self, reference: &str) -> Option<&str> {
        if reference.trim().is_empty() {
            return None;
        }
        // Keep validator-approved spellings even when html_make_id()
        // replaces their punctuation. Exact proof does not license a match
        // for a different spelling that happens to normalize the same way.
        if let Some(exact) = self.exact_native.get(reference) {
            return exact.as_deref();
        }
        if !self.native_word_keys.is_empty()
            && let Some(target) = self
                .native_word_keys
                .get(native_word_key(reference).as_ref())
        {
            return target.as_deref();
        }
        if let Some(exact) = self.authored.get(reference) {
            return exact.as_deref();
        }
        let mut candidates = self
            .authored
            .iter()
            .filter(|(title, _)| is_parenthetical_qualification(reference, title));
        let (_, candidate) = candidates.next()?;
        // An ambiguous authored title still occupies a candidate slot.
        if candidates.next().is_some() {
            return None;
        }
        candidate.as_deref()
    }
}

fn register_name(targets: &mut HashMap<String, Option<String>>, name: String, id: String) {
    targets
        .entry(name)
        .and_modify(|target| *target = None)
        .or_insert(Some(id));
}

fn native_word_key(source: &str) -> Cow<'_, str> {
    if !source.bytes().any(|byte| matches!(byte, b' ' | 28)) {
        return Cow::Borrowed(source);
    }
    // post_section() substitutes '_' for authored word separators even when
    // the same tag contains punctuation. Keep that punctuation as evidence
    // rather than folding different spellings to html_make_id()'s fragment.
    Cow::Owned(
        source
            .chars()
            .map(|ch| match ch {
                ' ' => '_',
                '\u{1c}' => '-', // mandoc.h's ASCII_HYPH
                other => other,
            })
            .collect(),
    )
}

fn is_parenthetical_qualification(reference: &str, title: &str) -> bool {
    let Some(suffix) = title.strip_prefix(reference) else {
        return false;
    };
    let Some(body) = suffix
        .trim_start_matches([' ', '\t'])
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
    else {
        return false;
    };
    if body.trim().is_empty() {
        return false;
    }
    let mut nested = 0usize;
    for byte in body.bytes() {
        match byte {
            b'(' => nested += 1,
            b')' if nested == 0 => return false,
            b')' => nested -= 1,
            _ => {}
        }
    }
    nested == 0
}

/// `mdoc_validate.c::post_section()` and `html.c::html_make_id()` both call
/// `roff.c::deroff()`. Walk operands, including parser-inserted default name
/// text. Generated terminal words (like `Fl`'s dash or `Xr`'s parentheses)
/// never participate in this namespace.
#[must_use]
pub fn authored_section_phrase(nodes: &[Node]) -> String {
    fn append(output: &mut String, nodes: &[Node]) {
        for node in nodes {
            if let Some(source) = &node.text {
                if let Some((first, subsequent)) = deroff_text_fragment(source) {
                    if output.is_empty() {
                        output.push_str(first);
                    } else {
                        // deroff uses %*s (minimum width) on later fragments,
                        // retaining their suffix after checking for emptiness.
                        output.push(' ');
                        output.push_str(subsequent);
                    }
                }
            } else {
                append(output, &node.children);
            }
        }
    }

    let mut output = String::new();
    append(&mut output, nodes);
    output
}

fn deroff_text_fragment(source: &str) -> Option<(&str, &str)> {
    let bytes = source.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] == b'\\'
            && bytes
                .get(start + 1)
                .is_some_and(|next| b" %&0^|~".contains(next))
        {
            start += 2;
        } else if bytes[start].is_ascii_whitespace() {
            start += 1;
        } else {
            break;
        }
    }

    let mut end = bytes.len();
    if end > start && bytes[end - 1] == b'\\' {
        end -= 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    (start < end).then(|| (&source[start..end], &source[start..]))
}

/// Independently establish whether an authored `Sx` can submit any label
/// glyphs to the formatter. Empty and control-only words still execute and
/// may affect state. Following words can overstrike a submitted glyph.
#[must_use]
pub fn section_label_has_glyphs(nodes: &[Node]) -> bool {
    section_label_evidence(nodes) != SectionLabelEvidence::Invisible
}

/// Distinguish native glyph evidence from readable escape recovery.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SectionLabelEvidence {
    /// At least one printable native operand is present.
    Native,
    /// The only printable operand comes from escape recovery.
    RecoveryOnly,
    /// No printable operand was found.
    Invisible,
}

/// Classify the label before judging whether a parser error makes it unproved.
#[must_use]
pub fn section_label_evidence(nodes: &[Node]) -> SectionLabelEvidence {
    let mut evidence = SectionLabelEvidence::Invisible;
    for node in nodes {
        if node.kind != NodeKind::Text {
            // Native Sx currently has a single TEXT child. Keep uncertainty
            // visible to the audit if another native operand shape appears.
            return SectionLabelEvidence::Native;
        }
        if let Some(source) = node.decoder_text() {
            for event in decode(source) {
                if let RoffInlineEvent::FallbackGlyph(value) = event {
                    if !value.is_empty() {
                        evidence = SectionLabelEvidence::RecoveryOnly;
                    }
                    continue;
                }
                if is_native_section_glyph(&event) {
                    return SectionLabelEvidence::Native;
                }
            }
        }
    }
    evidence
}

fn is_native_section_glyph(event: &RoffInlineEvent) -> bool {
    match event {
        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value)
        | RoffInlineEvent::Overstrike {
            terminal: Some(value),
            ..
        } => !value.is_empty(),
        RoffInlineEvent::NonBreakingWordBlank
        | RoffInlineEvent::BreakableHyphen
        | RoffInlineEvent::DeviceName
        | RoffInlineEvent::EmptyDestination => true,
        RoffInlineEvent::Overstrike { terminal: None, .. }
        | RoffInlineEvent::HorizontalMotion { .. }
        | RoffInlineEvent::ZeroAdvance
        | RoffInlineEvent::ZeroWidthGlyph
        | RoffInlineEvent::Breakpoint
        | RoffInlineEvent::Font(_)
        | RoffInlineEvent::PreviousFont
        | RoffInlineEvent::Link(_)
        | RoffInlineEvent::LineBreak
        | RoffInlineEvent::NoSpace
        | RoffInlineEvent::Presentation { .. }
        | RoffInlineEvent::FallbackGlyph(_) => false,
    }
}

mod canonical;
pub(super) use canonical::PendingSectionIdentity;
#[cfg(test)]
pub(super) use canonical::section_id_base;

#[cfg(test)]
mod tests;
