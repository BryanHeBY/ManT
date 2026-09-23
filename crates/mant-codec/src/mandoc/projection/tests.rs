use std::collections::HashSet;

use libmandoc_rs::structured::{
    ContentAtomKind, NativeLinkTarget, StructuredProfile, StructuredRenderer,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{Provenance, SourceIdentity, SourceKey};

use super::{NativeInlineLeaf, NativeProseProjection, project_native_prose};

fn bundle(entries: &[(&str, &[u8])]) -> SourceBundle {
    let mut bundle = SourceBundle::new();
    for (name, source) in entries {
        bundle.insert(*name, source.to_vec()).unwrap();
    }
    bundle
}

#[test]
fn prose_projection_resolves_style_and_link_runs_in_the_one_atom_store() {
    // The exact input was run first with the registered UTF-8/width-78 oracle.
    // Pinned `man_term.c::print_man_node` and `term.c::term_flushln` establish
    // the visible prose; `.UR` is wrapped by `man_term.c::pre_UR/post_UR`.
    let input = b".TH PROBE 1\n.SH NAME\n.B alpha\n.I beta\n.UR https://example.test\nlink\n.UE\n";
    let projection =
        project_native_prose("probe.1", &bundle(&[("probe.1", input)]), InputFormat::Man)
            .expect("C02b prose is completely supported");

    assert_eq!(projection.root_source(), SourceKey::FIRST);
    assert_eq!(projection.sources().len(), 1);
    assert!(!projection.blocks().is_empty());
    assert!(!projection.roots().is_empty());
    assert!(!projection.spans().is_empty());
    assert!(!projection.provenances().is_empty());

    let linked_runs = projection
        .roots()
        .iter()
        .flat_map(|root| &root.runs)
        .filter_map(|run| run.link)
        .collect::<HashSet<_>>();
    assert_eq!(linked_runs.len(), 1, "one authored link occurrence");

    let alpha = projection
        .roots()
        .iter()
        .flat_map(|root| {
            root.runs
                .iter()
                .flat_map(|run| root.run_leaves(run).unwrap())
        })
        .copied()
        .find(|leaf| projection.resolve_leaf(*leaf) == Some("alpha"))
        .expect("strong prose atom");
    let NativeInlineLeaf::Text(alpha_key) = alpha else {
        panic!("alpha is text")
    };
    let owned_text = projection
        .document()
        .content_atom(alpha_key)
        .and_then(|atom| atom.kind().logical_text())
        .unwrap();
    let projected_text = projection.resolve_leaf(alpha).unwrap();
    assert_eq!(projected_text.as_ptr(), owned_text.as_ptr());
    assert_eq!(projected_text.len(), owned_text.len());

    assert!(projection.roots().iter().any(|root| {
        root.runs.iter().any(|run| {
            run.style.is_bold()
                && root
                    .run_leaves(run)
                    .unwrap()
                    .iter()
                    .copied()
                    .any(|leaf| projection.resolve_leaf(leaf) == Some("alpha"))
        })
    }));
    assert!(projection.roots().iter().any(|root| {
        root.runs.iter().any(|run| {
            run.style.is_italic()
                && root
                    .run_leaves(run)
                    .unwrap()
                    .iter()
                    .copied()
                    .any(|leaf| projection.resolve_leaf(leaf) == Some("beta"))
        })
    }));
}

#[test]
fn included_atoms_keep_their_own_projected_source_key() {
    // The exact two-file bundle was run first with the registered oracle.
    // Pinned `read.c::mparse_readmem` restores include identity, and
    // `man_term.c::print_man_node` carries that source into accepted text.
    let root = b".TH ROOT 1\n.SH ROOT\nroot word\n.so include.1\nroot tail\n";
    let included = b".SH INCLUDED\nincluded word\n";
    let projection = project_native_prose(
        "root.1",
        &bundle(&[("root.1", root), ("include.1", included)]),
        InputFormat::Man,
    )
    .expect("included prose is supported");

    assert_eq!(projection.sources().len(), 2);
    assert!(matches!(
        &projection.sources()[1].identity,
        SourceIdentity::BundleMember { name } if name == "include.1"
    ));
    let included_atom = projection
        .document()
        .content_atoms()
        .iter()
        .find(|atom| {
            matches!(
                atom.kind(),
                ContentAtomKind::Text { text, .. } if text.contains("included")
            )
        })
        .expect("included atom");
    let provenance = &projection.provenances()[included_atom.provenance().get() as usize - 1];
    assert!(matches!(
        provenance,
        Provenance::Authored { span } if span.source == SourceKey::new(2).unwrap()
    ));
}

#[test]
fn mdoc_link_keeps_one_typed_occurrence_without_copying_its_target_or_label() {
    // Registered-oracle preflight (binary d7c587...e7311d) preceded the exact
    // UTF-8/78 input. Pinned `mdoc_term.c::termp_lk_pre` supplies the link and
    // `term.c::term_flushln` commits its styled label atoms.
    let input = b".Dd September 20, 2026\n.Dt LINK 1\n.Os\n.Sh NAME\n.Nm link\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.com label\n";
    let projection =
        project_native_prose("link.1", &bundle(&[("link.1", input)]), InputFormat::Mdoc)
            .expect("mdoc prose and Lk are supported");

    let occurrences = projection
        .roots()
        .iter()
        .flat_map(|root| &root.runs)
        .filter_map(|run| run.link)
        .collect::<HashSet<_>>();
    assert_eq!(occurrences.len(), 1);
    let occurrence = *occurrences.iter().next().unwrap();
    assert!(matches!(
        projection.document().link(occurrence).unwrap().target(),
        NativeLinkTarget::External(target) if target == "https://example.com"
    ));
    assert!(projection.roots().iter().any(|root| {
        root.runs.iter().any(|run| {
            run.link == Some(occurrence)
                && run.style.is_italic()
                && root
                    .run_leaves(run)
                    .unwrap()
                    .iter()
                    .copied()
                    .filter_map(|leaf| projection.resolve_leaf(leaf))
                    .collect::<String>()
                    == "label"
        })
    }));
}

#[test]
fn logical_connection_atoms_survive_the_final_private_projection() {
    // Registered oracle preflights preceded both exact probes: ASCII/78 for
    // `\~` and `\:`, then UTF-8/78 for `\p` and `.br`. Pinned
    // `term.c::term_word/term_fill/term_flushln` owns these distinctions.
    let connections = b".TH CONNECTIONS 1\n.SH TEST\nleft\\~right a\\:b\n";
    let connections_bundle = bundle(&[("connections.1", connections)]);
    let ascii = StructuredRenderer::new()
        .with_profile(StructuredProfile::Ascii)
        .render_bundle("connections.1", &connections_bundle, InputFormat::Man)
        .expect("ASCII logical connections");
    let ascii = NativeProseProjection::new(ascii).expect("connection projection");
    let ascii_leaves = ascii
        .roots()
        .iter()
        .flat_map(|root| &root.leaves)
        .copied()
        .collect::<Vec<_>>();
    assert!(
        ascii_leaves
            .iter()
            .any(|leaf| matches!(leaf, NativeInlineLeaf::BreakOpportunity(_)))
    );
    assert!(ascii_leaves.iter().copied().any(|leaf| {
        let atom = ascii.document().content_atom(leaf.atom()).unwrap();
        matches!(
            atom.kind(),
            ContentAtomKind::Whitespace {
                text,
                display_override: Some(display),
                breakable: false,
            } if text == "\u{a0}" && display == " "
        )
    }));
    assert!(ascii_leaves.iter().copied().any(|leaf| {
        matches!(
            ascii.document().content_atom(leaf.atom()).unwrap().kind(),
            ContentAtomKind::Whitespace { text, .. } if text == " "
        )
    }));

    let breaks = b".TH BREAK 1 \"2026-09-20\"\n.SH NAME\nbefore\\pafter\nmid\n.br\ntail\n";
    let hard = project_native_prose("break.1", &bundle(&[("break.1", breaks)]), InputFormat::Man)
        .expect("hard breaks are supported");
    let hard_breaks = hard
        .roots()
        .iter()
        .flat_map(|root| &root.leaves)
        .copied()
        .filter(|leaf| matches!(leaf, NativeInlineLeaf::HardBreak(_)))
        .collect::<Vec<_>>();
    assert!(hard_breaks.len() >= 2);
    assert!(
        hard_breaks
            .iter()
            .copied()
            .all(|leaf| hard.resolve_leaf(leaf) == Some("\n"))
    );
}

#[test]
fn c03_native_definition_structure_is_no_longer_a_partial_prose_failure() {
    // Registered-oracle preflight (binary d7c587...e7311d) and the exact
    // UTF-8/78 `.TP` probe preceded this assertion. Pinned
    // `man_term.c::pre_TP` treats it as a definition, not plain prose.
    let input = b".TH UNSUPPORTED 1\n.SH OPTIONS\n.TP\n.B -x\nbody\n";
    let projection = project_native_prose(
        "unsupported.1",
        &bundle(&[("unsupported.1", input)]),
        InputFormat::Man,
    )
    .expect("C03 projects complete native definition structure");
    assert_eq!(projection.document().lists().len(), 1);
    assert_eq!(projection.document().items().len(), 1);
    assert_eq!(projection.document().forms().len(), 1);
}
