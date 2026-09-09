use super::*;

#[test]
fn lowers_documented_mdoc_delimiters_and_common_roff_characters() {
    let path = temporary_source(
        "mdoc-delimiters",
        ".Dd July 19, 2026\n\
         .Dt DELIMITERS 7\n\
         .Os\n\
         .Sh DESCRIPTION\n\
         .Op optional\n\
         .Bq bracket\n\
         .Dq double\n\
         .Sq single\n\
         .Pq parenthesized\n\
         .Brq braced\n\
         .Aq angled\n\
         .Oo multi Ar value\n\
         .Oc\n\
         .Sh CHARACTERS\n\
         \\(en \\(em \\(aq \\(dq \\(co \\(rg \\(tm \\(bu \\(ha \\(ti \\(rs\n",
    );

    let document = parse_manual_source(&path).expect("lower delimiter and character source");
    fs::remove_file(path).expect("remove temporary roff fixture");

    let description = document.sections[0]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => inline_text(children),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    for expected in [
        "[optional]",
        "[bracket]",
        "“double”",
        "‘single’",
        "(parenthesized)",
        "{braced}",
        "<angled>",
        "[multi value]",
    ] {
        assert!(
            description.contains(expected),
            "missing {expected:?} in {description:?}"
        );
    }

    let [Block::Paragraph { children, .. }] = document.sections[1].blocks.as_slice() else {
        panic!("expected one special-character paragraph");
    };
    assert_eq!(inline_text(children), "– — ' \" © ® ™ • ^ ~ \\");
}

#[test]
fn lowers_the_pinned_named_character_catalog_without_silent_deletion() {
    let document = parse_manual_bytes(
        std::path::Path::new("named-characters.7"),
        b".TH NAMED-CHARACTERS 7\n\
.SH TEST\n\
at=\\(at ga=\\(ga oq=\\(oq arrow=\\(-> larrow=\\(<- mu=\\(mu\n\
de=\\(de pl=\\(pl dg=\\(dg ua=\\(ua da=\\(da lB=\\(lB rB=\\(rB\n\
unknown=\\[future-glyph]\n",
    )
    .expect("lower named characters");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one character paragraph");
    };
    assert_eq!(
        inline_text(children),
        "at=@ ga=` oq=' arrow=→ larrow=← mu=× de=° pl=+ dg=† ua=↑ da=↓ lB=[ rB=] unknown=\\[future-glyph]"
    );
}

#[test]
fn round_trips_raw_and_bracketed_unicode_manual_text() {
    let source = ".TH UNICODE 7\n\
.SH TEST\n\
Raw UTF-8: Mašláňová café — naïve.\n\
Escaped: Ma\\[u0161]l\\[u00E1] and \\[u2014] dash.\n";
    let document = parse_manual_bytes(std::path::Path::new("unicode.7"), source.as_bytes())
        .expect("lower raw and escaped Unicode");

    let [Block::Paragraph { children, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("expected one Unicode paragraph");
    };
    let rendered = inline_text(children);
    assert!(rendered.contains("Raw UTF-8: Mašláňová café — naïve."));
    assert!(rendered.contains("Escaped: Mašlá and — dash."));
    assert!(!rendered.contains(r"\[u"));
}
