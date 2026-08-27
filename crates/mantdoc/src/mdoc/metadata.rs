use super::{
    DocumentBuilder, NodeId, Recovery, SourceSpan, StructureOutcome, default_volume, node_arguments,
};

pub(super) fn record_date(
    builder: &mut DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) {
    let values = node_arguments(builder, node);
    let date = values.join(" ");
    let location = builder
        .children(node)
        .and_then(|children| children.first().copied())
        .and_then(|argument| builder.node_location(argument));
    if date.is_empty() {
        outcome.recoveries.push(Recovery::DateMissing {
            location: builder.node_location(node),
        });
    } else if legacy_man_date(&date) {
        outcome.recoveries.push(Recovery::LegacyDate {
            date: date.clone().into_boxed_str(),
            location,
        });
    } else if !is_mdoc_date(&date) {
        outcome.recoveries.push(Recovery::DateUnparseable {
            date: date.clone().into_boxed_str(),
            location,
        });
    }
    builder.metadata_mut().date = Some(normalize_mdoc_date(&date).into_boxed_str());
}

pub(super) fn is_mdoc_date(value: &str) -> bool {
    let value = value.trim();
    if value == "$Mdocdate$" {
        return true;
    }
    let value = value
        .strip_prefix("$Mdocdate: ")
        .and_then(|value| value.strip_suffix(" $"))
        .unwrap_or(value);
    let mut fields = value.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_month) else {
        return false;
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return false;
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<i32>().ok()) else {
        return false;
    };
    fields.next().is_none() && valid_calendar_day(month, day, year)
}

pub(super) fn legacy_man_date(value: &str) -> bool {
    let mut fields = value.split('-');
    let (Some(year), Some(month), Some(day)) = (fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    if fields.next().is_some() {
        return false;
    }
    if year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) =
        (year.parse::<i32>(), month.parse::<u8>(), day.parse::<u8>())
    else {
        return false;
    };
    let month = match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => return false,
    };
    valid_calendar_day(month, day, year)
}

/// Normalize the deterministic mdoc(7) date spellings accepted by mandoc.
///
/// `$Mdocdate$` intentionally remains literal: libmandoc expands that form
/// using wall-clock time, while native parsing must not consult host time.
pub(super) fn normalize_mdoc_date(value: &str) -> String {
    let value = value.trim();
    let value = value
        .strip_prefix("$Mdocdate: ")
        .and_then(|value| value.strip_suffix(" $"))
        .unwrap_or(value);
    let mut fields = value.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_month) else {
        return value.to_owned();
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return value.to_owned();
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<i32>().ok()) else {
        return value.to_owned();
    };
    if fields.next().is_some() || !valid_calendar_day(month, day, year) {
        return value.to_owned();
    }
    format!("{month} {day}, {year:04}")
}

pub(super) fn normalize_month(value: &str) -> Option<&'static str> {
    match value.get(..3)?.to_ascii_lowercase().as_str() {
        "jan" => Some("January"),
        "feb" => Some("February"),
        "mar" => Some("March"),
        "apr" => Some("April"),
        "may" => Some("May"),
        "jun" => Some("June"),
        "jul" => Some("July"),
        "aug" => Some("August"),
        "sep" => Some("September"),
        "oct" => Some("October"),
        "nov" => Some("November"),
        "dec" => Some("December"),
        _ => None,
    }
}

pub(super) fn valid_calendar_day(month: &str, day: u8, year: i32) -> bool {
    let maximum = match month {
        "January" | "March" | "May" | "July" | "August" | "October" | "December" => 31,
        "April" | "June" | "September" | "November" => 30,
        "February" if year.rem_euclid(4) != 0 => 28,
        "February" if year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0 => 29,
        "February" => 28,
        _ => return false,
    };
    (1..=maximum).contains(&day)
}

pub(super) fn record_title(
    builder: &mut DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) {
    let values = node_arguments(builder, node);
    if let Some((title, location)) = title_lowercase(builder, node) {
        outcome.recoveries.push(Recovery::TitleNotUppercase {
            title,
            location: Some(location),
        });
    }
    if let Some(argument) = values.get(3) {
        let location = builder
            .children(node)
            .and_then(|children| children.get(3).copied())
            .and_then(|argument_node| builder.node_location(argument_node));
        outcome.recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping excess arguments: Dt ... {argument}").into(),
            location,
        });
    }
    let location = builder.node_location(node);
    let title = values
        .first()
        .filter(|title| !title.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            outcome.recoveries.push(Recovery::MissingTitleArgument {
                location: location.clone(),
            });
            "UNTITLED".into()
        });
    let section = values.get(1).cloned();
    let volume = match section.as_deref() {
        Some(section) if let Some(volume) = default_volume(section) => volume.into_boxed_str(),
        Some(section) => {
            let location = builder
                .children(node)
                .and_then(|children| children.get(1).copied())
                .and_then(|argument| builder.node_location(argument));
            outcome.recoveries.push(Recovery::UnknownTitleSection {
                section: section.into(),
                location,
            });
            section.into()
        }
        None => {
            outcome.recoveries.push(Recovery::MissingTitleSection {
                title: title.clone().into_boxed_str(),
                location,
            });
            "LOCAL".into()
        }
    };
    let metadata = builder.metadata_mut();
    metadata.title = Some(title.into_boxed_str());
    metadata.section = section.map(String::into_boxed_str);
    metadata.volume = Some(volume);
    metadata.arch = values
        .get(2)
        .map(|value| value.to_ascii_lowercase().into_boxed_str());
}

pub(super) fn title_lowercase(
    builder: &DocumentBuilder,
    title: NodeId,
) -> Option<(Box<str>, SourceSpan)> {
    let argument = builder.children(title)?.first().copied()?;
    let title = builder.node_text(argument)?;
    let offset = title.bytes().position(|byte| byte.is_ascii_lowercase())?;
    let location = builder.node_location(argument)?;
    let offset = u32::try_from(offset).ok()?;
    let start = location.start.checked_add(offset)?;
    let location = SourceSpan::new(location.source, start, start.saturating_add(1)).ok()?;
    Some((title.to_owned().into_boxed_str(), location))
}

pub(super) fn record_operating_system(builder: &mut DocumentBuilder, node: NodeId) {
    let values = node_arguments(builder, node);
    if !values.is_empty() {
        builder.operating_system(values.join(" "));
    }
}

pub(super) fn mdoc_operating_system_flavour(value: &str) -> &'static str {
    if value.contains("OpenBSD") {
        "OpenBSD"
    } else {
        // libmandoc retains the historical NetBSD validation label for an
        // arbitrary explicit `.Os` value, while only literal `NetBSD`
        // activates its Mdocdate/RCS companion checks.
        "NetBSD"
    }
}

pub(super) fn record_name(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(value) = node_arguments(builder, node).into_iter().next() else {
        return;
    };
    if builder.metadata_mut().name.is_none() {
        // `.Nm` text keeps formatter spelling in the public AST, but document
        // metadata is the normalized lookup name.  In particular, `\\&` is a
        // zero-width no-break control and must not leak into `metadata.name`.
        builder.metadata_mut().name = Some(value.replace("\\&", "").into_boxed_str());
    }
}

pub(super) fn mark_no_print(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_print = true;
    let _ = builder.set_node_flags(node, flags);
}

/// Propagate the formatter's synopsis presentation state without relying on
/// ambient formatter globals.  The structural pass uses an explicit stack so
/// a malformed but bounded tree never consumes the process stack.
pub(super) fn mark_synopsis_pretty(builder: &mut DocumentBuilder, node: NodeId) {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if let Some(mut flags) = builder.node_flags(node) {
            flags.synopsis_pretty = true;
            let _ = builder.set_node_flags(node, flags);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

/// The execution-driven `nS` path differs subtly from `Sh SYNOPSIS`: a
/// generated fallback name remains generated prose rather than synopsis
/// presentation, even though its surrounding Nm block is synopsis-pretty.
pub(super) fn clear_generated_synopsis_pretty_children(
    builder: &mut DocumentBuilder,
    node: NodeId,
) {
    let children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    for child in children {
        let Some(mut flags) = builder.node_flags(child) else {
            continue;
        };
        if flags.generated {
            flags.synopsis_pretty = false;
            let _ = builder.set_node_flags(child, flags);
        }
    }
}

pub(super) fn mark_sentence_end(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(text) = builder.node_text(node) else {
        return;
    };
    let terminal = text.trim_end_matches(['"', '\'', ')', ']', '}']);
    if !terminal.ends_with(['.', '!', '?']) {
        return;
    }
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.sentence_end = true;
    let _ = builder.set_node_flags(node, flags);
}
