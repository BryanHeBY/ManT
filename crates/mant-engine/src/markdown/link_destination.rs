//! Single-pass URI encoding at the Markdown/logical-address boundary.

pub(super) fn decode_fragment(value: &str) -> Option<String> {
    let decoded = decode_component(value)?;
    (!decoded.is_empty()).then_some(decoded)
}

pub(super) fn decode_path(value: &str) -> Option<String> {
    value
        .split('/')
        .map(|part| {
            let decoded = decode_component(part)?;
            (!decoded.contains(['/', '\\', '?', '#'])).then_some(decoded)
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
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

pub(crate) fn encode_fragment(value: &str) -> String {
    encode(value, false)
}

pub(crate) fn document_destination(name: &str, fragment: Option<&str>) -> String {
    let mut destination = format!("{}.md", encode(name, true));
    if let Some(fragment) = fragment {
        destination.push('#');
        destination.push_str(&encode_fragment(fragment));
    }
    destination
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
