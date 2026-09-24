//! Query-local navigation for native Fixed section evidence.
//!
//! Visible search and explanation use the same checked section reader. This
//! module only converts its root/breadcrumb identities into protocol trails;
//! neither caller's matching, pagination, or copy budget lives here.

use std::num::NonZeroU32;

use mant_ir::{DOCUMENT_ROOT_ID, FixedSectionReader, OutlinePath};
use mant_protocol::{OutlineNodeReference, OutlineReference, OutlineTrail};

pub(crate) fn root_trail() -> OutlineTrail {
    OutlineTrail {
        ancestors: Vec::new(),
        node: OutlineNodeReference::DocumentRoot {
            path: OutlinePath::DocumentRoot.to_string().into(),
            id: DOCUMENT_ROOT_ID.into(),
            title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
        },
    }
}

pub(crate) fn section_trail(
    reader: &FixedSectionReader<'_>,
    section: Option<NonZeroU32>,
) -> Option<OutlineTrail> {
    let Some(section) = section else {
        return Some(root_trail());
    };
    let chain = reader.breadcrumbs(section)?;
    let mut ancestors = vec![OutlineReference {
        path: OutlinePath::DocumentRoot.to_string().into(),
        id: DOCUMENT_ROOT_ID.into(),
        title: crate::selectors::DOCUMENT_ROOT_TITLE.to_owned(),
    }];
    for heading in chain.iter().take(chain.len().saturating_sub(1)) {
        ancestors.push(OutlineReference {
            path: reader.path(heading.key)?.to_string().into(),
            id: heading.id.clone(),
            title: reader.label(heading.key)?,
        });
    }
    let heading = chain.last()?;
    Some(OutlineTrail {
        ancestors,
        node: OutlineNodeReference::DocumentSection {
            path: reader.path(section)?.to_string().into(),
            id: heading.id.clone(),
            title: reader.label(section)?,
        },
    })
}
