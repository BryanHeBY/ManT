use super::super::{Accent, Context, MAX_BYTES, Role, accents};
use super::*;

const CONTEXTS: [Context; 3] = [Context::Inferred, Context::Synopsis, Context::Entry];

fn counted(value: &str, names: &[Range<usize>]) -> Option<Syntax> {
    let mut uri = UriLookahead::default();
    let syntax = tokenize_with_lookahead(value, names, &mut uri);
    assert!(
        uri.scheme_bytes_examined <= value.len(),
        "URI scheme lookahead visited {} bytes for {} input bytes",
        uri.scheme_bytes_examined,
        value.len()
    );
    syntax
}

fn covers(value: &str, result: &[Accent], needle: &str, role: Role) -> bool {
    let start = value.find(needle).unwrap();
    result
        .iter()
        .any(|accent| accent.role == role && accent.bytes.contains(&start))
}

#[test]
fn scheme_lookahead_is_linear_across_repeats_and_complete_groups() {
    for (atom, sizes) in [
        ("...", &[2_048, 4_096, 8_192, 16_384][..]),
        ("a...", &[2_048, 4_096, 8_192, 12_288, 16_384]),
        ("[file]", &[2_048, 4_096, 8_192, 16_384]),
        ("[-f FILE]", &[2_048, 4_096, 8_192, 16_384]),
    ] {
        for &size in sizes {
            let value = format!("tool [-q] [--output FILE] [{}]", atom.repeat(size));
            let start = value.find("--output").unwrap();
            let name = start..start + "--output".len();
            let bytes = value.len();
            for names in [&[][..], std::slice::from_ref(&name)] {
                if bytes <= MAX_BYTES {
                    let syntax = counted(&value, names).unwrap();
                    assert!(syntax.tokens.len() >= size, "{atom}: {size} repeats");
                }
                for context in CONTEXTS {
                    let result = accents(&value, context, &[], names);
                    if bytes > MAX_BYTES {
                        assert!(result.is_empty(), "{atom}: {bytes} bytes");
                    } else {
                        assert!(covers(&value, &result, "FILE", Role::Placeholder));
                        assert_eq!(
                            covers(&value, &result, "--output", Role::Option),
                            names.is_empty(),
                            "{atom}: {bytes} bytes, {context:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn owner_byte_and_group_depth_limits_keep_their_boundaries() {
    let prefix = "tool [-q] [FILE ";
    for bytes in [MAX_BYTES - 1, MAX_BYTES, MAX_BYTES + 1] {
        let value = format!("{prefix}{}]", ".".repeat(bytes - prefix.len() - 1));
        assert_eq!(value.len(), bytes);
        if bytes <= MAX_BYTES {
            assert!(counted(&value, &[]).is_some());
        }
        for context in CONTEXTS {
            let result = accents(&value, context, &[], &[]);
            if bytes <= MAX_BYTES {
                assert!(covers(&value, &result, "FILE", Role::Placeholder));
            } else {
                assert_eq!(result, [], "{bytes} bytes, {context:?}");
            }
        }
    }
    for depth in [MAX_DEPTH, MAX_DEPTH + 1] {
        let value = format!("tool [-q] {}FILE{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(counted(&value, &[]).is_some(), depth == MAX_DEPTH);
        for context in CONTEXTS {
            let result = accents(&value, context, &[], &[]);
            if depth == MAX_DEPTH {
                assert!(covers(&value, &result, "FILE", Role::Placeholder));
            } else {
                assert_eq!(result, [], "depth {depth}, {context:?}");
            }
        }
    }
}

#[test]
fn uri_prefixes_stay_opaque_across_scheme_runs_and_protected_names() {
    for (source, protected, uri) in [
        ("https://a.b/", "", "https://a.b/"),
        ("a...b://host/", "", "a...b://host/"),
        ("a...b://host/", "a", "...b://host/"),
        ("pré...b://host/", "pré", "...b://host/"),
        ("a...b:other://host/", "", "other://host/"),
        ("<https://[::1]/?q=[x]>", "", "https://[::1]/?q=[x]"),
        ("(https://[::1]/?q=(x))", "", "https://[::1]/?q=(x)"),
        ("{https://[::1]/?q={x}}", "", "https://[::1]/?q={x}"),
    ] {
        let value = format!("tool [-q] [opérande] [{source}] [FILE]");
        let name_start = value.find(source).unwrap();
        let name = name_start..name_start + protected.len();
        let names = if protected.is_empty() {
            &[]
        } else {
            std::slice::from_ref(&name)
        };
        let syntax = counted(&value, names).unwrap();
        let opaque = syntax
            .tokens
            .iter()
            .find(|token| token.kind == Kind::Opaque)
            .expect("URI remains an opaque atom");
        assert_eq!(&value[opaque.bytes.clone()], uri);

        let start = value.find("opérande").unwrap();
        let end = start + "opérande".len();
        for split in (start..=end).filter(|&byte| value.is_char_boundary(byte)) {
            let emphasis: Vec<_> = [start..split, split..end]
                .into_iter()
                .filter(|range| !range.is_empty())
                .collect();
            for context in CONTEXTS {
                let result = accents(&value, context, &emphasis, names);
                assert!(result.iter().all(|accent| {
                    accent.bytes.end <= opaque.bytes.start || accent.bytes.start >= opaque.bytes.end
                }));
                // The existing inference rejects all rows that contain
                // an ordinary parenthesis group; the URI inside stays opaque.
                if !source.starts_with('(') || context == Context::Entry {
                    assert!(covers(&value, &result, "opérande", Role::Placeholder));
                    assert!(covers(&value, &result, "FILE", Role::Placeholder));
                }
            }
        }
    }
}
