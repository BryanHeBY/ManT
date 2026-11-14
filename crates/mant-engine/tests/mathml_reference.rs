//! Fixed-CVS eqn HTML branches through the public producer and Markdown export.
#![cfg(feature = "roff")]

use libmandoc_rs::{RenderFormat, Renderer};
use mant_codec::encode::{
    MarkdownMath, MarkdownMathIssue, MarkdownMathIssueKind, MarkdownOptions,
    render_addressable_markdown_with_options,
};
use mant_codec::math::MathLossKind;
use pulldown_cmark::{Event, Parser};

struct Outputs {
    native: String,
    markdown: String,
    issues: Vec<MarkdownMathIssue>,
}

fn outputs(source: &str) -> Outputs {
    let native = Renderer::new(RenderFormat::Html)
        .with_html_fragment(true)
        .render_bytes("probe.3", source.as_bytes())
        .unwrap()
        .output;
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let before = content.clone();
    assert_eq!(
        mant_ir::validate_document(content.document.as_ref().unwrap()),
        []
    );
    let artifact = render_addressable_markdown_with_options(
        &content,
        MarkdownOptions {
            math: MarkdownMath::Mathml,
            ..MarkdownOptions::default()
        },
    );
    let count = math_elements(&native).len();
    assert_eq!(artifact.math_summary().formulas, count, "{source}");
    assert_eq!(artifact.math_summary().encoded_formulas, count, "{source}");
    assert_eq!(artifact.math_summary().fallback_formulas, 0, "{source}");
    assert_eq!(math_elements(artifact.text()).len(), count, "{source}");
    let issues = artifact.math_issues().to_vec();
    let markdown = artifact.into_text();
    assert_eq!(content, before);
    Outputs {
        native,
        markdown,
        issues,
    }
}

fn formula(expression: &str) -> Outputs {
    outputs(&format!(
        ".TH PROBE 3\n.SH DESCRIPTION\n.EQ\n{expression}\n.EN\n"
    ))
}

fn math_elements(output: &str) -> Vec<&str> {
    output
        .split_inclusive("</math>")
        .filter_map(|part| {
            let start = part.find("<math ")?;
            assert!(part.ends_with("</math>"), "unterminated formula: {part}");
            Some(&part[start..])
        })
        .collect()
}

fn single_math(output: &str) -> &str {
    let elements = math_elements(output);
    assert_eq!(elements.len(), 1, "{output}");
    elements[0]
}

// Retain every token's exact payload, attributes and structure in the outputs.
// The small inspection helpers do not normalize text, unwrap groups or decode
// entities. These fixtures have no nested tables or markup inside token text.
fn atom_payloads(math: &str) -> Vec<&str> {
    math.split('<')
        .filter_map(|part| {
            let (opening, payload) = part.split_once('>')?;
            let name = opening.split_whitespace().next()?;
            matches!(name, "mi" | "mn" | "mo" | "mtext").then_some(payload)
        })
        .collect()
}

// These generated fragments have paired tags and XML-escaped token content.
// Keep nesting and text exact while excluding attributes checked separately.
fn element_structure(math: &str, element: &str) -> String {
    let mut signature = String::new();
    let mut stack = Vec::new();
    let mut started = false;
    for part in math.split('<').skip(1) {
        let (opening, payload) = part.split_once('>').expect("complete tag");
        let name = opening.split_whitespace().next().expect("tag name");
        if !started {
            if name != element {
                continue;
            }
            started = true;
        }
        signature.push('<');
        signature.push_str(name);
        signature.push('>');
        if let Some(closing) = name.strip_prefix('/') {
            assert_eq!(stack.pop(), Some(closing), "{math}");
            if stack.is_empty() {
                return signature;
            }
        } else {
            stack.push(name);
        }
        if matches!(name, "mi" | "mn" | "mo" | "mtext") {
            signature.push_str(payload);
        } else {
            assert!(payload.trim().is_empty(), "text outside a token: {part}");
        }
    }
    panic!("missing or unclosed {element}: {math}");
}

fn rows(math: &str) -> Vec<Vec<Vec<&str>>> {
    math.split("<mtr>")
        .skip(1)
        .map(|row| {
            row.split_once("</mtr>")
                .unwrap()
                .0
                .split("<mtd>")
                .skip(1)
                .map(|cell| atom_payloads(cell.split_once("</mtd>").unwrap().0))
                .collect()
        })
        .collect()
}

#[test]
fn every_native_position_keeps_its_tag_and_ordered_operands() {
    // Every exact TH PROBE 3 source ran with the fixed -Thtml oracle first.
    // eqn_html.c:96-123 selects these tags; eqn.c reparents ordered operands.
    for (source, tag, operands) in [
        ("a sup b", "msup", vec!["a", "b"]),
        ("a sub b", "msub", vec!["a", "b"]),
        ("a sub b sup c", "msubsup", vec!["a", "b", "c"]),
        ("a to b", "mover", vec!["a", "b"]),
        ("a from b", "munder", vec!["a", "b"]),
        ("a from b to c", "munderover", vec!["a", "b", "c"]),
        ("a over b", "mfrac", vec!["a", "b"]),
        ("sqrt a", "msqrt", vec!["a"]),
    ] {
        let output = formula(source);
        let native = single_math(&output.native);
        let core = single_math(&output.markdown);
        assert!(native.contains(&format!("<{tag}>")), "{source}: {native}");
        assert!(core.contains(&format!("<{tag}")), "{source}: {core}");
        assert!(core.contains(&format!("</{tag}>")), "{source}: {core}");
        assert_eq!(atom_payloads(native), operands, "{source}");
        assert_eq!(atom_payloads(core), operands, "{source}");
        let expected = format!("<{tag}><mi>{}</mi></{tag}>", operands.join("</mi><mi>"));
        assert_eq!(element_structure(native, tag), expected, "{source}");
        assert_eq!(element_structure(core, tag), expected, "{source}");
        assert_eq!(output.issues, [], "{source}");
        if matches!(tag, "mover" | "munderover") {
            assert!(core.contains("accent=\"false\""));
        }
        if matches!(tag, "munder" | "munderover") {
            assert!(core.contains("accentunder=\"false\""));
        }
    }
}

#[test]
fn native_decorations_keep_their_symbols_nesting_and_stretch_policy() {
    // eqn.c:728-795 creates nested unary owners for consecutive decorations.
    // Fixed HTML confirms bottom/top ordering. Core adds explicit accent and
    // stretch attributes and writes Unicode where native HTML uses entities.
    for (source, native_symbol, symbol, stretchy) in [
        ("x hat", "^", "^", true),
        ("x dot", "&#x02D9;", "˙", false),
        ("x dotdot", "&#x00A8;", "¨", false),
        ("x bar", "&#x203E;", "‾", true),
        ("x tilde", "~", "~", true),
        ("x vec", "&#x2192;", "→", true),
        ("x dyad", "&#x2194;", "↔", true),
        ("x under", "_", "&#95;", true),
    ] {
        let output = formula(source);
        let tag = if source == "x under" {
            "munder"
        } else {
            "mover"
        };
        assert!(single_math(&output.native).contains(&format!("<{tag}>")));
        assert_eq!(
            atom_payloads(single_math(&output.native)),
            ["x", native_symbol]
        );
        let core = single_math(&output.markdown);
        assert_eq!(atom_payloads(core), ["x", symbol]);
        assert_eq!(
            element_structure(single_math(&output.native), tag),
            format!("<{tag}><mi>x</mi><mo>{native_symbol}</mo></{tag}>")
        );
        assert_eq!(
            element_structure(core, tag),
            format!("<{tag}><mrow><mi>x</mi></mrow><mo>{symbol}</mo></{tag}>")
        );
        assert!(core.contains(&format!("stretchy=\"{stretchy}\"")));
        assert_eq!(output.issues, []);
    }
    for (source, outside, inside, native_symbols, core_symbols) in [
        (
            "x hat under",
            "munder",
            "mover",
            ["x", "^", "_"],
            ["x", "^", "&#95;"],
        ),
        (
            "x under hat",
            "mover",
            "munder",
            ["x", "_", "^"],
            ["x", "&#95;", "^"],
        ),
    ] {
        let output = formula(source);
        let native = single_math(&output.native);
        let core = single_math(&output.markdown);
        assert!(native.contains(&format!("<{outside}><{inside}>")));
        assert!(
            core.find(&format!("<{outside}")).unwrap() < core.find(&format!("<{inside}")).unwrap()
        );
        assert_eq!(atom_payloads(native), native_symbols);
        assert_eq!(atom_payloads(core), core_symbols);
        assert_eq!(
            element_structure(native, outside),
            format!(
                "<{outside}><{inside}><mi>x</mi><mo>{}</mo></{inside}><mo>{}</mo></{outside}>",
                native_symbols[1], native_symbols[2]
            )
        );
        assert_eq!(
            element_structure(core, outside),
            format!(
                "<{outside}><mrow><{inside}><mrow><mi>x</mi></mrow><mo>{}</mo></{inside}></mrow><mo>{}</mo></{outside}>",
                core_symbols[1], core_symbols[2]
            )
        );
    }
}

#[test]
fn native_fonts_and_atom_roles_survive_core_attribute_translation() {
    // eqn_html.c:148-199 elides default fonts and uses legacy fontstyle/weight.
    // Exact fixed reference runs establish every case. Core expresses resolved
    // atom fonts with normal mathvariant plus CSS, retaining inherited changes.
    for (source, native_atom, style, weight) in [
        ("x", "<mi>x</mi>", "italic", "normal"),
        (
            "roman x",
            "<mi fontstyle=\"normal\">x</mi>",
            "normal",
            "normal",
        ),
        ("italic x", "<mi>x</mi>", "italic", "normal"),
        ("bold x", "<mi fontweight=\"bold\">x</mi>", "normal", "bold"),
        ("fat x", "<mi fontweight=\"bold\">x</mi>", "normal", "bold"),
    ] {
        let output = formula(source);
        assert!(single_math(&output.native).contains(native_atom));
        assert!(single_math(&output.markdown).contains(&format!(
            "style=\"font-style:{style};font-weight:{weight}\">x</mi>"
        )));
        assert_eq!(output.issues, []);
    }
    let inherited = formula("roman { x italic y }");
    assert_eq!(atom_payloads(single_math(&inherited.native)), ["x", "y"]);
    assert!(
        inherited
            .markdown
            .contains("font-style:normal;font-weight:normal\">x</mi>")
    );
    assert!(
        inherited
            .markdown
            .contains("font-style:italic;font-weight:normal\">y</mi>")
    );
    for source in ["123", ".5"] {
        let output = formula(source);
        assert!(single_math(&output.native).contains(&format!("<mn>{source}</mn>")));
        assert!(single_math(&output.markdown).contains(&format!(">{source}</mn>")));
    }
    let greek = formula("alpha");
    assert!(greek.native.contains("<mo>&#x03B1;</mo>"));
    // The resolved identifier role corrects native ASCII-only classification.
    assert!(single_math(&greek.markdown).contains(">α</mi>"));
    let literal = formula("\"a < & >\"");
    assert_eq!(
        atom_payloads(single_math(&literal.native)),
        ["a &lt; &amp; &gt;"]
    );
    assert_eq!(
        atom_payloads(single_math(&literal.markdown)),
        ["a&#xA0;&lt;&#xA0;&amp;&#xA0;&gt;"]
    );
    let empty = formula("\"\"");
    assert_eq!(atom_payloads(single_math(&empty.native)), [""]);
    assert_eq!(atom_payloads(single_math(&empty.markdown)), [""]);
}

#[test]
fn quoted_spaces_survive_mathml_token_normalization() {
    // Pristine CVS eqn.c keeps quoted bytes. eqn_html.c::eqn_box emits them
    // through html.c::print_encode, whose print_endword collapses repeated
    // blanks. The exact input below produced ` a b ` in the reference HTML
    // before this assertion. Explicit NBSPs retain the original four blanks.
    let output = formula("\" a  b \"");
    assert_eq!(atom_payloads(single_math(&output.native)), [" a b "]);
    assert_eq!(
        atom_payloads(single_math(&output.markdown)),
        ["&#xA0;a&#xA0;&#xA0;b&#xA0;"]
    );
    assert_eq!(output.issues, []);
}

#[test]
fn digit_prefixed_names_keep_the_native_identifier_role() {
    // Before this assertion, pristine eqn_html.c::eqn_box classified the
    // complete explicitly roman-scoped `330E_16` spelling as one mi token.
    let output = formula("roman 330E_16");
    assert_eq!(atom_payloads(single_math(&output.native)), ["330E_16"]);
    // Markdown protects the authored underscore with an equivalent entity.
    assert_eq!(
        atom_payloads(single_math(&output.markdown)),
        ["330E&#95;16"]
    );
    assert_eq!(output.issues, []);
}

#[test]
fn native_fences_become_explicit_core_fence_operators() {
    // Fixed reference eqn_html.c:200-209 uses mfenced. Core represents its
    // independent sides with mo elements, including intentionally absent sides.
    for (source, native_open, native_close, core_atoms) in [
        (
            "left ( a + b right )",
            "(",
            ")",
            vec!["(", "a", "+", "b", ")"],
        ),
        ("left \"\" x right \"\"", "", "", vec!["x"]),
        (
            "left ceiling x right floor",
            "&#x2308;",
            "&#x230B;",
            vec!["⌈", "x", "⌋"],
        ),
        // CVS stores this spelling literally; it is not an invisible-side token.
        (
            "left nothing x right nothing",
            "nothing",
            "nothing",
            vec!["nothing", "x", "nothing"],
        ),
    ] {
        let output = formula(source);
        let native = single_math(&output.native);
        assert!(native.contains(&format!(
            "<mfenced open=\"{native_open}\" close=\"{native_close}\">"
        )));
        let core = single_math(&output.markdown);
        assert!(!core.contains("mfenced"));
        assert_eq!(atom_payloads(core), core_atoms);
        assert_eq!(
            core.matches("fence=\"true\" stretchy=\"true\"").count(),
            if native_open.is_empty() { 0 } else { 2 }
        );
    }
}

#[test]
fn piles_keep_authored_rows_and_recovered_inline_content() {
    // eqn_html.c:135-146 distinguishes a genuine row List from a recovered
    // Pile without rows. All exact sources ran through fixed HTML first.
    for (source, count) in [
        ("pile { a above b }", 2),
        ("pile {}", 1),
        ("pile { a }", 1),
        ("pile", 0),
        ("pile x", 0),
        ("pile roman x", 0),
    ] {
        let output = formula(source);
        let native = single_math(&output.native);
        let core = single_math(&output.markdown);
        assert_eq!(rows(native).len(), count, "{source}");
        assert_eq!(rows(core), rows(native), "{source}");
        assert_eq!(core.contains("<mtable>"), count > 0, "{source}");
        assert_eq!(atom_payloads(core), atom_payloads(native), "{source}");
        assert_eq!(output.issues, []);
    }
    let recovered = formula("pile x above y above z");
    let native = single_math(&recovered.native);
    let core = single_math(&recovered.markdown);
    assert!(!native.contains("<mtable>"));
    assert!(core.contains("<mtable>"));
    // CVS emits orphaned mtr nodes after x. Retain their complete row owners
    // inside a valid table following x, without making x a synthetic first row.
    assert_eq!(rows(native), rows(core));
    assert_eq!(rows(core), [[vec!["y"]], [vec!["z"]]]);
    assert_eq!(atom_payloads(core), ["x", "y", "z"]);
    assert!(core.find(">x</mi>").unwrap() < core.find("<mtable>").unwrap());
}

#[test]
fn matrix_row_order_preserves_cells_beyond_the_native_first_height() {
    // eqn_html.c:53-94 transposes native column order but bounds rows by the
    // first column. Exact oracle runs establish the rectangular common path
    // and the omitted tail below; IR retains every later column's owned row.
    let regular = formula("matrix { lcol { a above b } rcol { c above d } }");
    assert_eq!(
        rows(single_math(&regular.native)),
        [[vec!["a"], vec!["c"]], [vec!["b"], vec!["d"]]]
    );
    assert_eq!(
        rows(single_math(&regular.markdown)),
        rows(single_math(&regular.native))
    );
    assert_eq!(regular.issues, []);
    let unequal = formula("matrix { lcol { a above b } rcol { c above d above e } }");
    assert_eq!(rows(single_math(&unequal.native)).len(), 2);
    assert_eq!(
        rows(single_math(&unequal.markdown)),
        [
            vec![vec!["a"], vec!["c"]],
            vec![vec!["b"], vec!["d"]],
            vec![vec![], vec!["e"]]
        ]
    );
    assert!(
        unequal
            .issues
            .iter()
            .any(|issue| issue.kind
                == MarkdownMathIssueKind::Encoding(MathLossKind::IrregularMatrix))
    );
    let empty = formula("matrix { lcol {} rcol { c above d } }");
    assert_eq!(rows(single_math(&empty.native)).len(), 1);
    assert_eq!(
        rows(single_math(&empty.markdown)),
        [vec![vec![], vec!["c"]], vec![vec![], vec!["d"]]]
    );
    let font = formula("matrix { lcol roman x rcol { y } }");
    assert_eq!(rows(single_math(&font.markdown)), [[vec!["x"], vec!["y"]]]);
    assert!(
        font.markdown
            .contains("font-style:normal;font-weight:normal\">x</mi>")
    );
    let empty_axes = formula("matrix {}");
    assert_eq!(
        atom_payloads(single_math(&empty_axes.native)),
        Vec::<&str>::new()
    );
    assert!(single_math(&empty_axes.markdown).contains("<mtable></mtable>"));
    for source in ["matrix", "matrix x"] {
        let unsupported = formula(source);
        assert!(
            unsupported.issues.iter().any(|issue| issue.kind
                == MarkdownMathIssueKind::Encoding(MathLossKind::UnsupportedMatrix))
        );
        assert!(single_math(&unsupported.markdown).contains("<mtext"));
        assert_eq!(
            atom_payloads(single_math(&unsupported.markdown)),
            [if source == "matrix" {
                "matrix()"
            } else {
                "matrix(x)"
            }]
        );
    }
}

#[test]
fn inline_and_display_owners_keep_their_surrounding_prose() {
    // The exact fixed HTML source has two math roots, first inside BEFORE /
    // AFTER prose and second before TAIL. roff_eqndelim resolves inline origin;
    // print_eqn emits no formula for the delimiter-setting-only EQ owner.
    let output = outputs(
        ".TH PROBE 3\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nBEFORE $a sub i$ AFTER\n.PP\n.EQ\nx over y\n.EN\nTAIL\n",
    );
    let native = math_elements(&output.native);
    let core = math_elements(&output.markdown);
    assert_eq!(atom_payloads(native[0]), ["a", "i"]);
    assert_eq!(atom_payloads(native[1]), ["x", "y"]);
    assert_eq!(atom_payloads(core[0]), ["a", "i"]);
    assert_eq!(atom_payloads(core[1]), ["x", "y"]);
    assert!(core[0].contains("display=\"inline\""));
    assert!(core[1].contains("display=\"block\""));
    let paragraphs = Parser::new(&output.markdown)
        .filter_map(|event| match event {
            Event::Text(text) => Some(text.into_string()),
            _ => None,
        })
        .collect::<String>();
    assert!(paragraphs.contains("BEFORE"));
    assert!(paragraphs.contains("AFTER"));
    assert!(paragraphs.contains("TAIL"));
    assert!(output.markdown.find("BEFORE").unwrap() < output.markdown.find(core[0]).unwrap());
    assert!(output.markdown.find(core[0]).unwrap() < output.markdown.find("AFTER").unwrap());
    assert!(output.markdown.find("AFTER").unwrap() < output.markdown.find(core[1]).unwrap());
    assert!(output.markdown.find(core[1]).unwrap() < output.markdown.find("TAIL").unwrap());
}
