//! Recovers semantic mdoc inline calls from raw roff source embedded in tbl cells.

use mant_ir::Inline;

use crate::mandoc::roff_escape::visible_text;

use super::source::roff_macro_arguments;
use super::{
    InlineBuilder, enclosure_marks, is_source_closing_punctuation, lower_external_link,
    parse_roff_text, plain_text, surround, text_node, wrap_emphasis, wrap_strong,
};

/// Recover one mdoc inline request from source text flattened by `tbl`.
///
/// libmandoc 1.14.6 exposes the textual payload of `T{ ... T}` cells but not
/// an AST for mdoc requests inside those cells. Treating that payload as roff
/// text leaks child macro names (`Ar`, `Ns`, `Op`) or drops their arguments.
/// This source adapter mirrors ordinary inline lowering for callable mdoc
/// macros. Enclosures own the remaining arguments, so nested punctuation and
/// semantic children keep the same relationship as in a normal mdoc line.
pub(in crate::mandoc) fn lower_source_mdoc_request(
    macro_name: &str,
    source: &str,
    default_name: Option<&str>,
) -> Option<Vec<Inline>> {
    is_source_mdoc_macro(macro_name)?;
    let arguments = roff_macro_arguments(source);
    let calls = if enclosure_marks(macro_name).is_some() {
        vec![SourceMdocCall {
            name: macro_name.to_owned(),
            arguments: Vec::new(),
            enclosed: Some(parse_source_mdoc_calls("No", &arguments)),
        }]
    } else {
        parse_source_mdoc_calls(macro_name, &arguments)
    };
    Some(render_source_mdoc_calls(&calls, default_name))
}

#[derive(Debug)]
struct SourceMdocCall {
    name: String,
    arguments: Vec<String>,
    enclosed: Option<Vec<Self>>,
}

fn parse_source_mdoc_calls(first: &str, arguments: &[String]) -> Vec<SourceMdocCall> {
    let mut calls = vec![SourceMdocCall {
        name: first.to_owned(),
        arguments: Vec::new(),
        enclosed: None,
    }];
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if is_source_mdoc_macro(argument).is_some() {
            if enclosure_marks(argument).is_some() {
                calls.push(SourceMdocCall {
                    name: argument.clone(),
                    arguments: Vec::new(),
                    enclosed: Some(parse_source_mdoc_calls("No", &arguments[index + 1..])),
                });
                break;
            }
            calls.push(SourceMdocCall {
                name: argument.clone(),
                arguments: Vec::new(),
                enclosed: None,
            });
        } else if let Some(call) = calls.last_mut() {
            call.arguments.push(argument.clone());
        }
        index += 1;
    }
    calls
}

fn render_source_mdoc_calls(calls: &[SourceMdocCall], default_name: Option<&str>) -> Vec<Inline> {
    let mut builder = InlineBuilder::new();
    for call in calls {
        match call.name.as_str() {
            "Ns" => builder.tighten_next_boundary(),
            "Sm" => builder.set_spacing(source_argument_text(&call.arguments).trim()),
            "Ap" => {
                builder.tighten_next_boundary();
                builder.append(text_node("'"));
                builder.tighten_next_boundary();
            }
            "Pf" => {
                builder.append(render_source_mdoc_call(call, default_name));
                builder.tighten_next_boundary();
            }
            _ => builder.append(render_source_mdoc_call(call, default_name)),
        }
    }
    builder.finish()
}

fn render_source_mdoc_call(call: &SourceMdocCall, default_name: Option<&str>) -> Vec<Inline> {
    let children = call.enclosed.as_ref().map_or_else(
        || parse_roff_text(&source_argument_text(&call.arguments)),
        |calls| render_source_mdoc_calls(calls, default_name),
    );
    match call.name.as_str() {
        "Nm" => wrap_strong(if children.is_empty() {
            default_name.map_or_else(Vec::new, text_node)
        } else {
            children
        }),
        "Fl" => {
            let mut content = text_node("-");
            content.extend(children);
            wrap_strong(content)
        }
        "Cm" | "Ic" | "Sy" | "B" | "SB" => wrap_strong(children),
        "Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "I" => wrap_emphasis(children),
        "Li" => vec![Inline::Code {
            value: plain_text(&children),
        }],
        "In" if !children.is_empty() => vec![Inline::Code {
            value: format!("#include <{}>", plain_text(&children)),
        }],
        "Xr" => source_manual_reference(&call.arguments),
        "Sx" if !children.is_empty() => vec![Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: plain_text(&children).trim().into(),
            },
            title: None,
            children,
        }],
        "Lk" => source_external_link(&call.arguments, false),
        "Mt" => source_external_link(&call.arguments, true),
        "Fn" => source_function(&call.arguments),
        name if enclosure_marks(name).is_some() => {
            let (opening, closing) = enclosure_marks(name).expect("matched enclosure macro");
            surround(opening, children, closing)
        }
        _ => children,
    }
}

fn source_manual_reference(arguments: &[String]) -> Vec<Inline> {
    let Some(name) = arguments.first().map(|value| visible_text(value)) else {
        return Vec::new();
    };
    if name.is_empty() {
        return Vec::new();
    }
    let section = arguments
        .get(1)
        .filter(|value| !is_source_closing_punctuation(value))
        .map(|value| visible_text(value));
    let trailing_start = usize::from(section.is_some()) + 1;
    let display = section
        .as_ref()
        .map_or_else(|| name.clone(), |section| format!("{name}({section})"));
    let mut output = vec![Inline::Link {
        target: mant_ir::LinkTarget::Manual {
            name,
            manual_section: section,
        },
        title: None,
        children: text_node(&display),
    }];
    output.extend(parse_roff_text(&source_argument_text(
        arguments.get(trailing_start..).unwrap_or_default(),
    )));
    output
}

fn source_external_link(arguments: &[String], email: bool) -> Vec<Inline> {
    let Some(destination) = arguments.first().map(|value| visible_text(value)) else {
        return Vec::new();
    };
    if destination.is_empty() {
        return Vec::new();
    }
    let label = parse_roff_text(&source_argument_text(
        arguments.get(1..).unwrap_or_default(),
    ));
    lower_external_link(destination, label, email)
}

fn source_function(arguments: &[String]) -> Vec<Inline> {
    let Some(name) = arguments.first() else {
        return Vec::new();
    };
    let mut output = wrap_strong(parse_roff_text(name));
    output.extend(text_node("("));
    for (index, argument) in arguments.iter().skip(1).enumerate() {
        if index > 0 {
            output.extend(text_node(", "));
        }
        output.extend(wrap_emphasis(parse_roff_text(argument)));
    }
    output.extend(text_node(")"));
    output
}

fn source_argument_text(arguments: &[String]) -> String {
    let mut output = String::new();
    for argument in arguments {
        if !output.is_empty()
            && !is_source_closing_punctuation(argument)
            && !output.ends_with(['(', '[', '{', '<'])
        {
            output.push(' ');
        }
        output.push_str(argument);
    }
    output
}

fn is_source_mdoc_macro(name: &str) -> Option<()> {
    matches!(
        name,
        "Ad" | "Ap"
            | "Aq"
            | "Ar"
            | "B"
            | "Bo"
            | "Bq"
            | "Bro"
            | "Brq"
            | "Cd"
            | "Cm"
            | "Do"
            | "Dq"
            | "Dv"
            | "Em"
            | "Er"
            | "Ev"
            | "Fa"
            | "Fl"
            | "Fn"
            | "Ft"
            | "I"
            | "Ic"
            | "In"
            | "Li"
            | "Lk"
            | "Ms"
            | "Mt"
            | "Nm"
            | "No"
            | "Ns"
            | "Oo"
            | "Op"
            | "Pa"
            | "Pf"
            | "Po"
            | "Pq"
            | "Ql"
            | "Qo"
            | "Qq"
            | "SB"
            | "Sm"
            | "So"
            | "Sq"
            | "Sx"
            | "Sy"
            | "Tn"
            | "Va"
            | "Vt"
            | "Xr"
    )
    .then_some(())
}

#[cfg(test)]
mod tests {
    use mant_ir::Inline;

    use super::{lower_source_mdoc_request, source_external_link};
    use crate::inline::plain_text;

    #[test]
    fn source_manual_references_keep_semantics_and_trailing_punctuation() {
        let nodes = lower_source_mdoc_request("Xr", "git 1 ,", None)
            .expect("recognized source mdoc request");

        assert_eq!(plain_text(&nodes), "git(1),");
        assert!(matches!(
            nodes.as_slice(),
            [
                Inline::Link {
                    target:
                        mant_ir::LinkTarget::Manual {
                            name,
                            manual_section: Some(section),
                        },
                    ..
                },
                Inline::Text { value },
            ] if name == "git" && section == "1" && value == ","
        ));
    }

    #[test]
    fn source_external_links_keep_an_unlabelled_target_visible_before_punctuation() {
        let nodes = source_external_link(
            &["https://example.test/books".to_owned(), ".".to_owned()],
            false,
        );

        assert_eq!(plain_text(&nodes), "https://example.test/books.");
        assert!(matches!(
            nodes.as_slice(),
            [
                Inline::Link {
                    target: mant_ir::LinkTarget::External { uri },
                    children,
                    ..
                },
                Inline::Text { value },
            ] if uri == "https://example.test/books"
                && plain_text(children) == "https://example.test/books"
                && value == "."
        ));
    }
}
