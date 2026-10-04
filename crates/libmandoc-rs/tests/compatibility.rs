//! Public compatibility switches, independently exercised without workspace
//! feature unification. Exact inputs were run with the registered pristine CVS
//! oracle before these assertions were added.

use libmandoc_rs::{EquationBox, Node, NodeKind, Parser};

const FONTS: &[u8] = b".TH PANDOC-FONTS 1\n.SH NAME\npandoc-fonts \\- fixture\n.SH DESCRIPTION\n\\f[C]code\\f[R] \\f[V]verbatim\\f[R] \\f[VB]bold\\f[R] \\f[VI]italic\\f[R]\n";
const STANDARD_FONTS: &[u8] = b".TH FONTS 1\n.SH DESCRIPTION\n\\f[CR]code\\f[R] \\f[CW]wide\\f[R] \\f[CB]bold\\f[R] \\f[CI]italic\\f[R]\n";
const LIBRARY: &[u8] = b".Dd August 19, 2026\n.Dt LIBBSD 3bsd\n.Os\n.Sh LIBRARY\n.Lb libbsd\n";
const EQUATIONS: &[u8] = b".TH EQN 7\n.SH DESCRIPTION\n.EQ\nldots\n.EN\n.EQ\n\"ldots\"\n.EN\n.EQ\nldots2\n.EN\n.EQ\ndefine xx /ldots/ xx\n.EN\n.EQ\ndefine yy /ldots2/ yy\n.EN\n";

fn find_macro<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.macro_token.as_deref() == Some(name) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| find_macro(child, name))
    }
}

fn equations<'a>(node: &'a Node, found: &mut Vec<&'a EquationBox>) {
    if node.kind == NodeKind::Equation {
        found.push(node.equation.as_ref().expect("owned equation"));
    }
    for child in &node.children {
        equations(child, found);
    }
}

fn ldots_atoms<'a>(node: &'a EquationBox, found: &mut Vec<&'a EquationBox>) {
    if node.text.as_deref() == Some("ldots") {
        found.push(node);
    }
    for child in &node.children {
        ldots_atoms(child, found);
    }
}

#[test]
fn pandoc_aliases_follow_the_native_font_capability() {
    // CVS mandoc.c::mandoc_font rejects C/V/VB/VI; roff_escape.c returns
    // ESCAPE_ERROR and validation diagnoses all four. The optional patch
    // admits these aliases without changing the standard font spellings.
    let report = Parser::default().parse_bytes("fonts.1", FONTS).unwrap();
    let invalid = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("invalid escape sequence"))
        .count();
    assert_eq!(
        invalid,
        if cfg!(feature = "compat-pandoc") {
            0
        } else {
            4
        }
    );
}

#[test]
fn standard_fonts_remain_available_without_compatibility() {
    let report = Parser::default()
        .parse_bytes("fonts.1", STANDARD_FONTS)
        .unwrap();
    assert!(
        report
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("invalid escape sequence"))
    );
}

#[test]
fn libbsd_catalogue_entry_follows_its_own_capability() {
    // CVS lib.c::mdoc_a2lib and mdoc_validate.c::post_lb expand known names,
    // otherwise preserve the name in library "..." and report unknown Lb.
    let report = Parser::default()
        .parse_bytes("libbsd.3bsd", LIBRARY)
        .unwrap();
    let library = find_macro(&report.document.root, "Lb").unwrap();
    let visible = library
        .children
        .iter()
        .filter(|child| !child.flags.no_print)
        .filter_map(|child| child.text.as_deref())
        .collect::<Vec<_>>();
    let unknown = report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("unknown library name"));
    if cfg!(feature = "compat-libbsd") {
        assert_eq!(
            visible,
            ["Utility functions from BSD systems (libbsd, \\-lbsd)"]
        );
        assert!(!unknown);
    } else {
        assert_eq!(visible, ["library", r"\(lq", "libbsd", r"\(rq"]);
        assert!(unknown);
    }
}

#[test]
fn gnu_equation_normalization_requires_enabled_complete_token_evidence() {
    // CVS eqn_next expands aliases and bypasses interpretation of quoted
    // tokens; eqn_parse then splits mixed-font atoms. Pristine keeps ldots.
    // The compatibility feature interprets only complete unquoted tokens.
    let report = Parser::default()
        .parse_bytes("equations.7", EQUATIONS)
        .unwrap();
    let mut parsed = Vec::new();
    equations(&report.document.root, &mut parsed);
    assert_eq!(parsed.len(), 5);
    let enabled = cfg!(feature = "compat-gnu-eqn");
    for (equation, (eligible, literal)) in parsed.iter().zip([
        (true, "ldots"),
        (false, "ldots"),
        (false, "ldots 2"),
        (true, "ldots"),
        (false, "ldots 2"),
    ]) {
        let mut atoms = Vec::new();
        ldots_atoms(equation, &mut atoms);
        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms[0].gnu_ldots, enabled && eligible);
        assert_eq!(
            equation.readable_text(),
            if enabled && eligible { "..." } else { literal }
        );
    }
}

#[cfg(feature = "render")]
#[test]
fn native_html_obeys_the_same_font_switch() {
    let render = |source| {
        libmandoc_rs::Renderer::new(libmandoc_rs::RenderFormat::Html)
            .render_bytes("fonts.1", source)
            .unwrap()
            .output
    };
    let paragraph_text = |html: &str| {
        // These exact inputs have plain ASCII labels. Read the last Pp,
        // excluding font tags, headers and CSS; no entity decoding is needed.
        let paragraph = html.rsplit_once("<p class=\"Pp\">").unwrap().1;
        let paragraph = paragraph.split_once("</p>").unwrap().0;
        let mut text = String::new();
        let mut in_tag = false;
        for character in paragraph.chars() {
            match character {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => text.push(character),
                _ => {}
            }
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let aliases = render(FONTS);
    assert_eq!(paragraph_text(&aliases), "code verbatim bold italic");
    assert_eq!(
        aliases.contains("<span class=\"Li\">code</span>"),
        cfg!(feature = "compat-pandoc")
    );
    assert_eq!(
        aliases.contains("<b>bold</b>"),
        cfg!(feature = "compat-pandoc")
    );
    assert_eq!(
        aliases.contains("<i>italic</i>"),
        cfg!(feature = "compat-pandoc")
    );
    let standard = render(STANDARD_FONTS);
    assert_eq!(paragraph_text(&standard), "code wide bold italic");
    assert!(standard.contains("<span class=\"Li\">code</span>"));
    assert!(standard.contains("<b>bold</b>"));
    assert!(standard.contains("<i>italic</i>"));
}

#[cfg(feature = "serde")]
#[test]
fn serialized_equations_cannot_enable_disabled_normalization() {
    let report = Parser::default()
        .parse_bytes("equations.7", EQUATIONS)
        .unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    let decoded: libmandoc_rs::ParseReport = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, report);
    let mut parsed = Vec::new();
    equations(&decoded.document.root, &mut parsed);
    let mut atoms = Vec::new();
    ldots_atoms(parsed[0], &mut atoms);
    // Public owned values can carry a flag from an enabled producer. The
    // receiving crate's feature still governs its readable projection.
    let mut foreign = atoms[0].clone();
    foreign.gnu_ldots = true;
    let encoded = serde_json::to_string(&foreign).unwrap();
    let foreign: EquationBox = serde_json::from_str(&encoded).unwrap();
    let expected = if cfg!(feature = "compat-gnu-eqn") {
        "..."
    } else {
        "ldots"
    };
    assert_eq!(foreign.normalized_text(), Some(expected));
    assert_eq!(foreign.readable_text(), expected);
}
