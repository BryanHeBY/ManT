//! Native search identity uses decoded and accepted word evidence.
use super::support::{assert_search_fixture, explain, load, source};
use mant_ir::{EntryOwner, inline_plain_text};
use mant_protocol::EvidenceBasis;

fn probe(head: &str) -> String {
    source("", ".Sh COMMANDS\n.Sm off\n", &format!("{head}\n"))
}

#[test]
fn native_escape_encodings_preserve_search_operand_roles() {
    // Pinned term.c::term_word() resolves SPECIAL, UNICODE and NUMBERED
    // before encode1() writes the sign or lets the Ar overwrite its cell.
    for (spelling, sign) in [
        (r"\(sl", '/'),
        (r"\[sl]", '/'),
        (r"\[u002F]", '/'),
        (r"\[u003F]", '?'),
        (r"\N'47'", '/'),
        (r"\N'63'", '?'),
        (r"\C'sl'", '/'),
        (r"\E(sl", '/'),
        (r"\[u002f]", '/'),
        (r"\N'047'", '/'),
        (r"\N!47!", '/'),
        (r"\C'u003F'", '?'),
        (r"\f[]/", '/'),
    ] {
        let input = probe(&format!(".It Pf {spelling} Ar RE"));
        assert_search_fixture(&input, &format!("{sign}RE"), &[], Some(1));
        let input = probe(&format!(r".It Bq Pf \z{spelling} Ar RE"));
        let content = assert_search_fixture(&input, "[RE]", &[], None);
        let argument = explain(&content, "RE");
        assert_eq!(argument.counts.direct_entry.total, 0, "{input}");
        assert!(argument.supports.is_empty(), "{input}");
    }
    for (head, form, group) in [
        (
            ".It Xo\n.Pf / Ar RE\n\\C'sl'\n.Ar OFFSET\n.Xc",
            "/RE/OFFSET",
            Some(1),
        ),
        (r".It Pf / Ar \[u0061]", "/a", Some(1)),
        (r".It Pf / Ar \(eu", "/€", Some(1)),
        (r".It No / Ar R\zX Ar E", "/RE", Some(1)),
        (r".It No / Ar \zXRE", "/RE", Some(1)),
        (r".It No / Ar \zR Ar E", "/E", None),
    ] {
        assert_search_fixture(&probe(head), form, &[], group);
    }
}

#[test]
fn malformed_operands_do_not_qualify_a_reading_group() {
    // roff_escape.c rejects these Unicode spellings, font selectors and
    // quoted special characters. Reading recovery can keep a glyph that the
    // native escape did not prove. The remaining form is still available.
    for (head, form) in [
        (r".It Pf \C'u0002F' Ar RE", "/RE"),
        (r".It Pf /\f[NOFONT] Ar RE", "/RE"),
        (r".It Pf /\C'' Ar RE", "/RE"),
        (r".It Pf /\? Ar RE", "/RE"),
        (r".It Pf /\[:] Ar RE", "/RE"),
        (r".It Pf / Ar \[u00061]", "/a"),
        (
            ".It Xo\n.Pf / Ar RE\n/\\f[NOFONT]\n.Ar OFFSET\n.Xc",
            "/RE/OFFSET",
        ),
    ] {
        assert_search_fixture(&probe(head), form, &[], None);
    }
}

#[test]
fn overwritten_argument_glyphs_cannot_name_the_visible_suffix() {
    // A bare BACKAFTER, including after an ignored or invalid character
    // escape, buffers the first Ar glyph. The following encode1() removes
    // that glyph: only 'E' of the authored 'RE' remains in the HEAD.
    for head in [
        r".It Bq Ar \zRE",
        r".It Bq No \z Ar RE",
        r".It Bq Pf \z\/ Ar RE",
        r".It Bq Pf \z\[unknown] Ar RE",
        r".It Bq Pf \z\N'2047' Ar RE",
    ] {
        let input = probe(head);
        let content = assert_search_fixture(&input, "[E]", &[], None);
        assert_eq!(
            explain(&content, "E").counts.direct_entry.total,
            0,
            "{input}"
        );
    }
    assert_search_fixture(&probe(r".It Bq Ar RE\z"), "[RE]", &["RE"], Some(0));
    assert_search_fixture(&probe(r".It Bq Ar RE"), "[RE]", &["RE"], Some(0));
    for head in [
        r".It Bq Ar \zRE , Cm later",
        r".It Bq Pf \z\[unknown] Ar RE , Cm later",
    ] {
        let input = probe(head);
        let content = assert_search_fixture(&input, "[E,later]", &["later"], Some(0));
        assert_eq!(
            explain(&content, "E").counts.direct_entry.total,
            0,
            "{input}"
        );
        assert_eq!(
            explain(&content, "later").counts.direct_entry.total,
            1,
            "{input}"
        );
    }
}

#[test]
fn identical_argument_and_declared_name_have_distinct_ranges() {
    let input = probe(r".It Bq Pf \z\(sl Ar RE , Cm RE");
    let content = assert_search_fixture(&input, "[RE,RE]", &["RE"], Some(0));
    let document = content.document.as_ref().unwrap();
    let items = super::definitions(document);
    let owner = EntryOwner::Definition(items[0]);
    let facts = owner.facts().unwrap();
    let [occurrence] = facts.name_bindings[0].occurrences.as_slice() else {
        panic!("only the independently declared RE can bind");
    };
    let [part] = occurrence.parts.as_slice() else {
        panic!("one slice for the declared RE");
    };
    assert_eq!(inline_plain_text(&owner.content_slice(part).unwrap()), "RE");
    assert_eq!(
        inline_plain_text(&items[0].terms[0].content[..part.path[0]]),
        "[RE,"
    );
    let result = explain(&content, "RE");
    assert_eq!(result.counts.direct_entry.total, 1);
    assert!(result.evidence.iter().any(|row| {
        row.bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    }));
}

#[test]
fn run_in_body_cannot_name_a_detached_partial_argument() {
    // mdoc_term.c::termp_it_pre() writes a nonbreaking blank for an inset
    // BODY. HEAD post leaves the buffer live. A pending 'E' passes the
    // owner boundary and may be overstruck there (term.c::encode1()).
    for (argument, form, names) in [
        (r"R\zE", "R", &[][..]),
        (r#""R\zE ""#, "R", &[][..]),
        (r#""\zR E""#, "RE", &["RE"][..]),
        ("RE", "RE", &["RE"][..]),
    ] {
        let input = probe(&format!(".It Ar {argument}\n.No BodyWord"))
            .replace(".Bl -tag -width Ds", ".Bl -inset");
        let content = load(&input);
        let items = super::definitions(content.document.as_ref().unwrap());
        assert_eq!(inline_plain_text(&items[0].terms[0]), form, "{input}");
        assert_eq!(
            EntryOwner::Definition(items[0]).validated_names().unwrap(),
            names,
            "{input}"
        );
        assert!(
            explain(&content, "R").evidence.iter().all(|row| row
                .bases
                .iter()
                .all(|basis| { !matches!(basis, EvidenceBasis::Name { .. }) })),
            "{input}"
        );
    }
}
