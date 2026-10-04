//! Inventory and wire contracts for the owned macro identity.

use super::{MacroToken, ManMacro, MdocMacro, RoffMacro};
use crate::{Node, NodeKind, Parser};
use std::collections::BTreeSet;

fn pinned_names() -> Vec<(&'static str, &'static str)> {
    let header = include_str!("../../vendor/mandoc-cvs-20260927T130954Z/roff.h");
    let source = include_str!("../../vendor/mandoc-cvs-20260927T130954Z/roff.c");
    let tokens = header
        .split_once("enum\troff_tok {")
        .unwrap()
        .1
        .split_once("};")
        .unwrap()
        .0
        .lines()
        .filter_map(|line| {
            let token = line
                .split_once("/*")
                .map_or(line, |(prefix, _)| prefix)
                .trim()
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .next()?;
            (!token.is_empty()).then_some(token)
        });
    let names: Vec<_> = source
        .split_once("const char *__roff_name[MAN_MAX + 1] = {")
        .unwrap()
        .1
        .split_once("};")
        .unwrap()
        .0
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let tokens: Vec<_> = tokens.collect();
    assert_eq!(tokens.len(), names.len());
    tokens
        .into_iter()
        .zip(names)
        .filter_map(|(token, name)| {
            // TOKEN_NONE has the diagnostic name "text", not a macro.
            if name == "NULL" || token == "TOKEN_NONE" {
                None
            } else {
                Some((
                    token,
                    name.strip_prefix('"').unwrap().strip_suffix('"').unwrap(),
                ))
            }
        })
        .collect()
}

#[test]
fn every_named_pinned_token_has_exactly_one_family_variant() {
    let inventory = pinned_names();
    let expected: BTreeSet<_> = inventory.iter().copied().collect();
    assert_eq!(expected.len(), 412);
    for (prefix, names) in [
        (
            "ROFF_",
            RoffMacro::ALL
                .iter()
                .map(|token| token.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "MAN_",
            ManMacro::ALL
                .iter()
                .map(|token| token.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "MDOC_",
            MdocMacro::ALL
                .iter()
                .map(|token| token.as_str())
                .collect::<Vec<_>>(),
        ),
    ] {
        let pinned: BTreeSet<_> = inventory
            .iter()
            .filter_map(|(token, name)| token.starts_with(prefix).then_some(*name))
            .collect();
        assert_eq!(
            names.len(),
            pinned.len(),
            "duplicate or missing {prefix} variant"
        );
        assert_eq!(names.into_iter().collect::<BTreeSet<_>>(), pinned);
    }
    for token in RoffMacro::ALL {
        assert_eq!(RoffMacro::from_name(token.as_str()), Some(*token));
    }
    for token in ManMacro::ALL {
        assert_eq!(ManMacro::from_name(token.as_str()), Some(*token));
    }
    for token in MdocMacro::ALL {
        assert_eq!(MdocMacro::from_name(token.as_str()), Some(*token));
    }
    for (_, name) in inventory {
        let token = MacroToken::from_name(name);
        assert!(token.is_known(), "missing canonical token {name}");
        assert_eq!(token.as_str(), name);
    }
}

#[test]
fn formatter_request_classification_tracks_the_native_node_boundary() {
    let header = include_str!("../../vendor/mandoc-cvs-20260927T130954Z/roff.h");
    let native_range = header
        .split_once("enum\troff_tok {")
        .unwrap()
        .1
        .split_once("ROFF_MAX")
        .unwrap()
        .0;
    let native_tokens: BTreeSet<_> = native_range
        .lines()
        .filter_map(|line| {
            let token = line
                .trim()
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .next()?;
            token.starts_with("ROFF_").then_some(token)
        })
        .collect();
    let expected: BTreeSet<_> = pinned_names()
        .into_iter()
        .filter_map(|(token, name)| native_tokens.contains(token).then_some(name))
        .collect();
    let actual: BTreeSet<_> = RoffMacro::ALL
        .iter()
        .filter(|token| token.generates_node())
        .map(|token| token.as_str())
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn macro_spelling_preserves_case_aliases_and_preprocessing_identity() {
    for (name, expected) in [
        ("Sh", MacroToken::Mdoc(MdocMacro::Sh)),
        ("SH", MacroToken::Man(ManMacro::Sh)),
        ("br", MacroToken::Roff(RoffMacro::Br)),
        ("BR", MacroToken::Man(ManMacro::Br)),
        ("MT", MacroToken::Man(ManMacro::Mt)),
        ("Mt", MacroToken::Mdoc(MdocMacro::Mt)),
        ("P", MacroToken::Man(ManMacro::P)),
        ("PP", MacroToken::Man(ManMacro::Pp)),
        ("LP", MacroToken::Man(ManMacro::Lp)),
        ("%A", MacroToken::Mdoc(MdocMacro::PercentA)),
        ("T&", MacroToken::Roff(RoffMacro::TAnd)),
        (".", MacroToken::Roff(RoffMacro::BlockEnd)),
        ("Dd", MacroToken::Mdoc(MdocMacro::Dd)),
        ("TH", MacroToken::Man(ManMacro::Th)),
    ] {
        assert_eq!(MacroToken::from_name(name), expected);
        assert_eq!(expected.to_string(), name);
    }
    assert_ne!(RoffMacro::Bp, RoffMacro::BP);
    assert_ne!(RoffMacro::Pi, RoffMacro::PI);
    assert_eq!(RoffMacro::from_name("Dd"), Some(RoffMacro::Dd));
    assert_eq!(
        MacroToken::from(RoffMacro::Dd),
        MacroToken::Roff(RoffMacro::Dd)
    );
    assert_eq!(
        MacroToken::from(ManMacro::Th),
        MacroToken::Man(ManMacro::Th)
    );
    assert_eq!(
        MacroToken::from(MdocMacro::Dd),
        MacroToken::Mdoc(MdocMacro::Dd)
    );
}

#[test]
fn unknown_names_remain_owned_and_known_names_are_static() {
    for name in [
        "",
        "FutureBlock",
        "text",
        "TOKEN_NONE",
        "ROFF_MAX",
        "sh",
        "中文",
    ] {
        let original = name.to_owned();
        let original_pointer = original.as_ptr();
        let token = MacroToken::from(original);
        assert_eq!(token, MacroToken::Unknown(name.to_owned()));
        assert!(!token.is_known());
        assert_eq!(
            token.as_str().as_ptr(),
            original_pointer,
            "reuse owned unknown storage"
        );
    }
    let token = MacroToken::from_name("Fo");
    let static_name: &'static str = MdocMacro::Fo.as_str();
    assert_eq!(token.as_str(), static_name);
    assert_eq!(Some(token).as_deref(), Some("Fo"));
}

const MAN_SOURCE: &[u8] = b".TH TOKENS 1\n.SH NAME\ntokens \\- typed AST\n.SH DESCRIPTION\n.nf\n.ft B\nSTYLE\n.fi\n.UR https://example.org\nlabel\n.UE\n";
const MDOC_SOURCE: &[u8] = b".Dd October 4, 2026\n.Dt TOKENS 1\n.Os TestOS\n.Sh NAME\n.Nm tokens\n.Nd typed AST\n.Sh DESCRIPTION\n.Bl -tag -width key\n.It Bq key\n.Fo call\n.Fa arg\n.Fc\n.El\n";

fn collect<'a>(node: &'a Node, tokens: &mut Vec<(NodeKind, Option<&'a MacroToken>)>) {
    tokens.push((node.kind, node.macro_token.as_ref()));
    for child in &node.children {
        collect(child, tokens);
    }
}

#[test]
fn owned_transfer_preserves_native_node_roles_and_typed_macro_families() {
    // Exact inputs were run through the fixed CVS reference with -Ttree,
    // -Tascii and -Tutf8 before these assertions. roff.c::roff_node_alloc()
    // assigns the same token to macro BLOCK/HEAD/BODY, and TOKEN_NONE to TEXT.
    for (source, expected) in [
        (
            MAN_SOURCE,
            vec![
                MacroToken::Man(ManMacro::Sh),
                MacroToken::Man(ManMacro::Ur),
                MacroToken::Roff(RoffMacro::Ft),
            ],
        ),
        (
            MDOC_SOURCE,
            vec![
                MacroToken::Mdoc(MdocMacro::Dd),
                MacroToken::Mdoc(MdocMacro::Sh),
                MacroToken::Mdoc(MdocMacro::Bq),
                MacroToken::Mdoc(MdocMacro::Fo),
            ],
        ),
    ] {
        let report = Parser::default().parse_bytes("tokens.1", source).unwrap();
        let mut nodes = Vec::new();
        collect(&report.document.root, &mut nodes);
        for token in expected {
            assert!(
                nodes.iter().any(|(_, found)| *found == Some(&token)),
                "missing {token:?}"
            );
        }
        for (kind, token) in nodes {
            if matches!(kind, NodeKind::Root | NodeKind::Text) {
                assert!(token.is_none(), "{kind:?} cannot acquire a named macro");
            } else if let Some(token) = token {
                assert!(token.is_known(), "native registry token became Unknown");
            }
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn string_wire_format_preserves_canonical_tokens_and_unknown_names() {
    for (_, name) in pinned_names() {
        let token = MacroToken::from_name(name);
        let encoded = serde_json::to_string(&token).unwrap();
        assert_eq!(encoded, serde_json::to_string(name).unwrap());
        assert_eq!(serde_json::from_str::<MacroToken>(&encoded).unwrap(), token);
    }
    for name in ["", "FutureBlock", "中文"] {
        let token = MacroToken::from_name(name);
        let encoded = serde_json::to_string(&token).unwrap();
        assert_eq!(serde_json::from_str::<MacroToken>(&encoded).unwrap(), token);
    }
    for token in RoffMacro::ALL {
        let encoded = serde_json::to_string(token).unwrap();
        assert_eq!(serde_json::from_str::<RoffMacro>(&encoded).unwrap(), *token);
    }
    for token in ManMacro::ALL {
        let encoded = serde_json::to_string(token).unwrap();
        assert_eq!(serde_json::from_str::<ManMacro>(&encoded).unwrap(), *token);
    }
    for token in MdocMacro::ALL {
        let encoded = serde_json::to_string(token).unwrap();
        assert_eq!(serde_json::from_str::<MdocMacro>(&encoded).unwrap(), *token);
    }
    assert!(serde_json::from_str::<ManMacro>("\"FutureBlock\"").is_err());
    assert!(serde_json::from_str::<MacroToken>("42").is_err());
}

#[cfg(feature = "serde")]
#[test]
fn document_json_keeps_macro_name_property_and_round_trips_after_parser_drop() {
    for source in [MAN_SOURCE, MDOC_SOURCE] {
        let mut report = Parser::default().parse_bytes("tokens.1", source).unwrap();
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(encoded.contains("\"macro_name\":\""));
        assert!(!encoded.contains("\"macro_token\""));
        assert_eq!(
            serde_json::from_str::<crate::ParseReport>(&encoded).unwrap(),
            report
        );
        report.document.root.macro_token = Some(MacroToken::from_name("FutureBlock"));
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(encoded.contains("\"macro_name\":\"FutureBlock\""));
        assert_eq!(
            serde_json::from_str::<crate::ParseReport>(&encoded).unwrap(),
            report
        );
    }
}
