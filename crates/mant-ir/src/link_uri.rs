//! Single-pass conversion between typed references and link destinations.

use crate::LinkTarget;

impl LinkTarget {
    /// Interpret a link destination without resolving it or consulting a host.
    ///
    /// Fragment, native `man:`, single-mailbox `mailto:`, and relative Markdown
    /// destinations become typed references. Other spellings remain external
    /// URIs, including malformed input that document validation must diagnose.
    /// URI components are decoded once, after their structural delimiters.
    #[must_use]
    pub fn from_uri(destination: &str) -> Self {
        if let Some(target) = destination.strip_prefix('#').and_then(decode_fragment) {
            Self::Section { id: target.into() }
        } else if let Some(target) = manual_reference(destination) {
            target
        } else if let Some(address) = crate::email_address_from_mailto_uri(destination) {
            Self::Email { address }
        } else if let Some((name, fragment)) = markdown_document_reference(destination) {
            Self::Document { name, fragment }
        } else {
            Self::External {
                uri: destination.to_owned(),
            }
        }
    }

    /// Serialize this exact target as a reusable link destination.
    ///
    /// Relative documents regain an equivalent canonical `.md` suffix; decoded path/fragment and
    /// manual-topic components are percent-encoded once. An omitted manual
    /// section remains omitted. External URIs retain their authored spelling.
    ///
    /// Returns `None` for malformed or unrepresentable targets rather than
    /// changing their type, dropping a component, or repairing their text.
    /// This does not grant permission to open the destination or guarantee
    /// that a relative document is within its eventual source namespace. Its
    /// address remains relative to the original document, not globally absolute.
    #[must_use]
    pub fn to_uri(&self) -> Option<String> {
        let destination = match self {
            Self::Document { name, fragment } => {
                let mut value = format!("{}.md", encode(name, true));
                if let Some(fragment) = fragment {
                    value.push('#');
                    value.push_str(&encode(fragment, false));
                }
                value
            }
            Self::Manual {
                name,
                manual_section,
            } => {
                let mut value = format!("man:{}", encode(name, false));
                if let Some(section) = manual_section {
                    value.push('(');
                    value.push_str(&encode(section, false));
                    value.push(')');
                }
                value
            }
            Self::Section { id } => format!("#{}", encode(id.as_str(), false)),
            Self::Email { address } => crate::mailto_uri_for_email_address(address)?,
            Self::External { uri } => {
                if !crate::is_valid_external_uri(uri) {
                    return None;
                }
                uri.clone()
            }
        };
        (Self::from_uri(&destination) == *self).then_some(destination)
    }
}

/// Parse a relative Markdown link into its extension-free logical name and
/// optional decoded fragment, without using platform filesystem conventions.
///
/// Parent components remain relative; source-namespace confinement is checked
/// when resolving the document. Encoded separators, controls, queries, and
/// absolute paths are not reinterpreted as document references.
#[must_use]
pub fn markdown_document_reference(destination: &str) -> Option<(String, Option<String>)> {
    if destination.split(['/', '#', '?']).next()?.contains(':') {
        return None;
    }
    let (path, fragment) = if let Some((path, fragment)) = destination.split_once('#') {
        (
            path,
            if fragment.is_empty() {
                None
            } else {
                Some(decode_fragment(fragment)?)
            },
        )
    } else {
        (destination, None)
    };
    let path = decode_path(path)?;
    if path.split('/').next()?.contains(':') {
        return None;
    }
    if path.contains(['\\', '?']) || path.starts_with('/') || path.chars().any(char::is_control) {
        return None;
    }
    let (parent, leaf) = path
        .rsplit_once('/')
        .map_or(("", path.as_str()), |(parent, leaf)| (parent, leaf));
    let (filename, extension) = leaf.rsplit_once('.')?;
    if !extension.eq_ignore_ascii_case("md") && !extension.eq_ignore_ascii_case("markdown") {
        return None;
    }
    if filename.is_empty() {
        return None;
    }
    let logical = if parent.is_empty() {
        filename.to_owned()
    } else {
        format!("{parent}/{filename}")
    };
    let valid = logical.split('/').all(|component| {
        !component.is_empty()
            && (matches!(component, "." | "..") || !component.chars().any(char::is_control))
    });
    valid.then_some((logical, fragment))
}

fn decode_fragment(value: &str) -> Option<String> {
    let decoded = decode_component(value)?;
    (!decoded.is_empty()).then_some(decoded)
}

fn decode_path(value: &str) -> Option<String> {
    value
        .split('/')
        .map(|part| {
            let decoded = decode_component(part)?;
            (!decoded.contains(['/', '\\', '?', '#'])).then_some(decoded)
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn manual_reference(value: &str) -> Option<LinkTarget> {
    let (scheme, value) = value.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("man") || value.contains(['/', '\\', '?', '#']) {
        return None;
    }
    let (name, section) = if let Some(without_close) = value.strip_suffix(')') {
        let (name, section) = without_close.rsplit_once('(')?;
        (name, Some(decode_component(section)?))
    } else {
        (value, None)
    };
    if name.contains(['(', ')']) {
        return None;
    }
    let name = decode_component(name)?;
    let reference = crate::DocumentReference::Manual {
        name: name.clone(),
        manual_section: section.clone(),
    };
    reference.is_well_formed().then_some(LinkTarget::Manual {
        name,
        manual_section: section,
    })
}

fn decode_component(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = char::from(input.next()?).to_digit(16)?;
            let low = char::from(input.next()?).to_digit(16)?;
            bytes.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            bytes.push(byte);
        }
    }
    let decoded = String::from_utf8(bytes).ok()?;
    (!decoded.chars().any(char::is_control)).then_some(decoded)
}

fn encode(value: &str, path: bool) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (path && byte == b'/')
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_target_components_round_trip_once() {
        for (input, canonical) in [
            ("target.md#details", "target.md#details"),
            ("target.markdown#Mixed%2ETarget", "target.md#Mixed.Target"),
            (
                "../a%20b/percent%252F.md#part%2528",
                "../a%20b/percent%252F.md#part%2528",
            ),
            ("man:demo%281%29", "man:demo%281%29"),
            ("man:demo%281%29(3)", "man:demo%281%29(3)"),
            ("man:a%2528b", "man:a%2528b"),
            ("man:printf(3p)", "man:printf(3p)"),
            ("#Mixed%2ETarget", "#Mixed.Target"),
            (
                "mailto:percent%25box@example.org",
                "mailto:percent%25box@example.org",
            ),
            (
                "https://example.org/a%2528?x=%20#frag",
                "https://example.org/a%2528?x=%20#frag",
            ),
        ] {
            let target = LinkTarget::from_uri(input);
            assert_eq!(target.to_uri().as_deref(), Some(canonical), "{input}");
            assert_eq!(LinkTarget::from_uri(canonical), target, "{input}");
        }
    }

    #[test]
    fn unrepresentable_targets_are_not_repaired_or_reclassified() {
        for target in [
            LinkTarget::Document {
                name: String::new(),
                fragment: None,
            },
            LinkTarget::Document {
                name: "absolute:topic".into(),
                fragment: None,
            },
            LinkTarget::Document {
                name: "/absolute".into(),
                fragment: None,
            },
            LinkTarget::Document {
                name: "part\\leaf".into(),
                fragment: None,
            },
            LinkTarget::Document {
                name: "part#leaf".into(),
                fragment: None,
            },
            LinkTarget::Document {
                name: "target".into(),
                fragment: Some(String::new()),
            },
            LinkTarget::Manual {
                name: "demo".into(),
                manual_section: Some("qgroup".into()),
            },
            LinkTarget::Manual {
                name: "bad\nname".into(),
                manual_section: None,
            },
            LinkTarget::Section {
                id: "bad\nfragment".into(),
            },
            LinkTarget::Email {
                address: "not a mailbox".into(),
            },
            LinkTarget::External {
                uri: "https://example.org/%xx".into(),
            },
            LinkTarget::External {
                uri: "man:demo(1)".into(),
            },
        ] {
            assert_eq!(target.to_uri(), None, "{target:?}");
        }
    }

    #[test]
    fn encoded_delimiters_are_not_used_to_escape_uri_classification() {
        for source in [
            "https://example.md",
            "//example.org/page.md",
            "x%3Ay.md",
            "part%2Fleaf.md",
            "part%5Cleaf.md",
            "part%23leaf.md",
            "part%3Fleaf.md",
            "part%00leaf.md",
            "part%FFleaf.md",
            "part%xxleaf.md",
            "part.md?query=1",
            "man:demo(qgroup)",
        ] {
            assert!(
                matches!(LinkTarget::from_uri(source), LinkTarget::External { .. }),
                "{source}"
            );
        }
    }
}
