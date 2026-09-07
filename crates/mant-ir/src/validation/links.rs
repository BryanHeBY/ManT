//! URI and mailbox syntax; independent of host activation policy.

/// Return whether an email link contains one conservative mailbox spelling.
///
/// The local part is one ASCII dot-atom and the domain uses conservative DNS
/// labels. Quoted strings, internationalized addresses, and address comments
/// remain outside the document contract.
#[must_use]
pub fn is_valid_email_address(address: &str) -> bool {
    if address.is_empty()
        || !address.is_ascii()
        || address
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || address.contains(['?', '#', ','])
    {
        return false;
    }
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    valid_dot_atom(local) && !domain.contains('@') && valid_email_domain(domain)
}

/// Decode one single-recipient `mailto:` URI into its typed email address.
///
/// Header fields, fragments, and recipient lists remain external URIs because
/// they cannot be represented by [`crate::LinkTarget::Email`]. Percent escapes are
/// decoded exactly once before the conservative ASCII mailbox is validated.
#[must_use]
pub fn email_address_from_mailto_uri(uri: &str) -> Option<String> {
    let (scheme, remainder) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("mailto") || remainder.contains(',') {
        return None;
    }
    let (recipient, query, fragment) = uri_components(remainder)?;
    if query.is_some() || fragment.is_some() {
        return None;
    }
    decode_mailto_recipient(recipient)
}

/// Serialize one typed email address as a structurally valid `mailto:` URI.
///
/// URI-sensitive characters in the ASCII dot-atom local part are percent-
/// encoded exactly once. Unsupported mailbox forms return `None` rather than
/// producing a target that a consumer would later reject.
#[must_use]
pub fn mailto_uri_for_email_address(address: &str) -> Option<String> {
    if !is_valid_email_address(address) {
        return None;
    }
    let mut uri = String::with_capacity("mailto:".len() + address.len());
    uri.push_str("mailto:");
    for byte in address.bytes() {
        if is_mailto_addr_spec_byte(byte) {
            uri.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            uri.push('%');
            uri.push(char::from(HEX[usize::from(byte >> 4)]));
            uri.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    Some(uri)
}

/// Return whether an absolute external URI satisfies the document contract.
///
/// Every component uses RFC 3986 ASCII characters and complete percent
/// triplets. HTTP(S) additionally validates authority, userinfo, host, IPv6,
/// and port structure. `mailto` requires conservative dot-atom mailboxes.
/// Host activation applies a narrower scheme allowlist separately.
#[must_use]
pub fn is_valid_external_uri(uri: &str) -> bool {
    let Some((scheme, remainder)) = uri.split_once(':') else {
        return false;
    };
    let syntax_valid = !remainder.is_empty()
        && scheme
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
        && uri.is_ascii()
        && !uri
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ');
    if !syntax_valid {
        return false;
    }
    let Some((hierarchy, query, fragment)) = uri_components(remainder) else {
        return false;
    };
    if query.is_some_and(|value| !valid_query_or_fragment(value))
        || fragment.is_some_and(|value| !valid_query_or_fragment(value))
    {
        return false;
    }
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        let Some(authority_and_path) = hierarchy.strip_prefix("//") else {
            return false;
        };
        let (authority, path) = authority_and_path
            .find('/')
            .map_or((authority_and_path, ""), |index| {
                authority_and_path.split_at(index)
            });
        return valid_http_authority(authority) && valid_path(path);
    }
    if scheme.eq_ignore_ascii_case("mailto") {
        return !hierarchy.is_empty()
            && hierarchy
                .split(',')
                .all(|recipient| decode_mailto_recipient(recipient).is_some());
    }
    valid_generic_hierarchy(hierarchy)
}

fn decode_mailto_recipient(recipient: &str) -> Option<String> {
    if recipient.is_empty() || !recipient.is_ascii() {
        return None;
    }
    let bytes = recipient.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = decode_hex(*bytes.get(index + 1)?)?;
            let low = decode_hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else if is_mailto_addr_spec_byte(bytes[index]) {
            decoded.push(bytes[index]);
            index += 1;
        } else {
            return None;
        }
    }
    let address = String::from_utf8(decoded).ok()?;
    (address.is_ascii() && is_valid_email_address(&address)).then_some(address)
}

const fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const fn is_mailto_addr_spec_byte(byte: u8) -> bool {
    is_unreserved(byte)
        || matches!(
            byte,
            b'!' | b'$' | b'\'' | b'(' | b')' | b'*' | b'+' | b':' | b'@'
        )
}

fn uri_components(remainder: &str) -> Option<(&str, Option<&str>, Option<&str>)> {
    let (before_fragment, fragment) = remainder
        .split_once('#')
        .map_or((remainder, None), |(value, fragment)| {
            (value, Some(fragment))
        });
    if fragment.is_some_and(|value| value.contains('#')) {
        return None;
    }
    let (hierarchy, query) = before_fragment
        .split_once('?')
        .map_or((before_fragment, None), |(value, query)| {
            (value, Some(query))
        });
    Some((hierarchy, query, fragment))
}

fn valid_generic_hierarchy(hierarchy: &str) -> bool {
    if let Some(authority_and_path) = hierarchy.strip_prefix("//") {
        let (authority, path) = authority_and_path
            .find('/')
            .map_or((authority_and_path, ""), |index| {
                authority_and_path.split_at(index)
            });
        return (authority.is_empty() || valid_http_authority(authority)) && valid_path(path);
    }
    valid_path(hierarchy)
}

fn valid_path(path: &str) -> bool {
    valid_uri_component(path, |byte| is_pchar(byte) || byte == b'/')
}

fn valid_query_or_fragment(value: &str) -> bool {
    valid_uri_component(value, |byte| is_pchar(byte) || matches!(byte, b'/' | b'?'))
}

fn valid_uri_component(value: &str, allowed: impl Fn(u8) -> bool) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else if allowed(bytes[index]) {
            index += 1;
        } else {
            return false;
        }
    }
    true
}

const fn is_pchar(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delimiter(byte) || matches!(byte, b':' | b'@')
}

const fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

const fn is_sub_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn valid_dot_atom(local: &str) -> bool {
    !local.is_empty()
        && local
            .split('.')
            .all(|atom| !atom.is_empty() && atom.bytes().all(is_atext))
}

const fn is_atext(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'/'
                | b'='
                | b'?'
                | b'^'
                | b'_'
                | b'`'
                | b'{'
                | b'|'
                | b'}'
                | b'~'
        )
}

fn valid_http_authority(authority: &str) -> bool {
    if authority.is_empty() || authority.contains('\\') {
        return false;
    }
    let host_port = if let Some((userinfo, host_port)) = authority.rsplit_once('@') {
        if userinfo.is_empty()
            || userinfo.contains('@')
            || !valid_uri_component(userinfo, |byte| {
                is_unreserved(byte) || is_sub_delimiter(byte) || byte == b':'
            })
        {
            return false;
        }
        host_port
    } else {
        authority
    };
    if let Some(bracketed) = host_port.strip_prefix('[') {
        let Some((host, suffix)) = bracketed.split_once(']') else {
            return false;
        };
        return host.parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    if host_port.contains(['[', ']']) {
        return false;
    }
    let (host, port) = host_port
        .rsplit_once(':')
        .map_or((host_port, None), |(host, port)| (host, Some(port)));
    valid_uri_reg_name(host) && port.is_none_or(valid_port)
}

fn valid_uri_reg_name(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && valid_uri_component(label, |byte| is_unreserved(byte) || is_sub_delimiter(byte))
        })
}

fn valid_email_domain(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok()
}
