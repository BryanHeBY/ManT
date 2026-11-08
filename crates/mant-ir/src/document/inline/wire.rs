//! Local inline decoding, including formula projection invariants in every DTO.
use super::{EquationExpression, Inline, LinkTarget, NodeId, SourceSpan};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    remote = "Inline",
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum InlineWire {
    Text {
        value: String,
    },
    Strong {
        children: Vec<Inline>,
    },
    Emphasis {
        children: Vec<Inline>,
    },
    Code {
        value: String,
    },
    Equation {
        value: String,
        expression: EquationExpression,
    },
    Link {
        target: LinkTarget,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        children: Vec<Inline>,
    },
    Anchor {
        id: NodeId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fragment_aliases: Vec<crate::FragmentAlias>,
        #[serde(skip_serializing_if = "Option::is_none")]
        owner_source: Option<SourceSpan>,
    },
    LineBreak {},
}

impl<'de> Deserialize<'de> for Inline {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let inline = InlineWire::deserialize(deserializer)?;
        if let Self::Equation { value, expression } = &inline {
            expression
                .validate_projection(value)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(inline)
    }
}
