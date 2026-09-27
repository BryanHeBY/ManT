//! Parse-local native component boundaries applied to final visible HEAD text.
//! The marker records an execution instance; only this module decides whether
//! its surviving glyphs make a complete name.

use std::ops::Range;

use crate::definitions::{NativeHeadComponent, NativeHeadRole, RecognizedName};

use super::named;

pub(super) fn configuration_names(
    text: &str,
    components: Option<&[NativeHeadComponent]>,
    arguments: Option<&[Range<usize>]>,
) -> Option<Vec<RecognizedName>> {
    let Some(components) = components else {
        let range = mant_ir::configuration_key_declaration_range(text)?;
        let name = text.get(range.clone())?;
        return Some(vec![RecognizedName::contiguous(name, range.start)]);
    };
    checked_components(
        text,
        components,
        arguments.unwrap_or_default(),
        NativeHeadRole::Literal,
        |component| {
            let range = mant_ir::configuration_key_declaration_range(component)?;
            Some((component.get(range.clone())?, range.start))
        },
    )
}

pub(super) fn variable_names(
    text: &str,
    components: &[NativeHeadComponent],
) -> Option<Vec<RecognizedName>> {
    checked_components(
        text,
        components,
        &[],
        NativeHeadRole::Variable,
        |component| {
            let start = component.len() - component.trim_start().len();
            let name = component.get(start..)?.trim_end();
            named::is_variable_term(name).then_some((name, start))
        },
    )
}

fn checked_components<'a>(
    text: &'a str,
    components: &[NativeHeadComponent],
    arguments: &[Range<usize>],
    role: NativeHeadRole,
    name: impl Fn(&'a str) -> Option<(&'a str, usize)>,
) -> Option<Vec<RecognizedName>> {
    if components.is_empty() || components.len() > 64 || arguments.len() > 64 {
        return None;
    }
    let mut names = Vec::with_capacity(components.len());
    let mut previous = 0;
    let mut argument_index = 0;
    for (index, component) in components.iter().enumerate() {
        if component.role != role || component.range.start < previous {
            return None;
        }
        checked_gap(
            text,
            previous..component.range.start,
            arguments,
            &mut argument_index,
            index != 0,
        )?;
        let visible = text.get(component.range.clone())?;
        let (spelling, local_start) = name(visible)?;
        names.push(RecognizedName::contiguous(
            spelling,
            component.range.start.checked_add(local_start)?,
        ));
        previous = component.range.end;
    }
    checked_gap(
        text,
        previous..text.len(),
        arguments,
        &mut argument_index,
        false,
    )?;
    (argument_index == arguments.len()).then_some(names)
}

fn checked_gap(
    text: &str,
    gap: Range<usize>,
    arguments: &[Range<usize>],
    argument_index: &mut usize,
    allow_separator: bool,
) -> Option<()> {
    let mut residual = String::new();
    let mut cursor = gap.start;
    while let Some(argument) = arguments.get(*argument_index) {
        if argument.start >= gap.end {
            break;
        }
        if argument.start < cursor || argument.end > gap.end {
            return None;
        }
        let before = text.get(cursor..argument.start)?;
        let raw_argument = text.get(argument.clone())?;
        // `Cm Batch Ns Ar Mode` is one joined native word, not key Batch
        // followed by a parameter. The first visible Ar glyph must have a
        // real separator from the preceding key or Ar instance, whether the
        // formatter placed that separator just before or inside the Ar span.
        if !before.chars().any(char::is_whitespace)
            && !raw_argument.chars().next().is_some_and(char::is_whitespace)
        {
            return None;
        }
        residual.push_str(before);
        let value = raw_argument.trim();
        if !mant_ir::native_argument_component_token(value) {
            return None;
        }
        cursor = argument.end;
        *argument_index += 1;
    }
    let tail = text.get(cursor..gap.end)?;
    residual.push_str(tail);
    let connector = if allow_separator {
        mant_ir::complete_literal_component_gap(&residual)
    } else {
        residual.trim().is_empty()
    };
    // Two Cm/Va macro instances joined by Ns are one displayed token. Even
    // though their native instances differ, an empty gap proves no second
    // declaration boundary. If Ar intervened, the separator must follow the
    // final argument rather than merely precede the first one.
    (connector && (!allow_separator || !tail.is_empty())).then_some(())
}
