use super::{DocumentBuilder, NodeId, SourceSpan};

pub(super) fn record_title_metadata(builder: &mut DocumentBuilder, title: NodeId) {
    let values = builder
        .children(title)
        .into_iter()
        .flatten()
        .filter_map(|argument| builder.node_text(*argument))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let metadata = builder.metadata_mut();
    metadata.title = Some(values.first().cloned().unwrap_or_default().into_boxed_str());
    metadata.section = Some(values.get(1).cloned().unwrap_or_default().into_boxed_str());
    metadata.date = Some(
        values
            .get(2)
            .map_or_else(String::new, |date| normalize_title_date(date))
            .into_boxed_str(),
    );
    metadata.os = values.get(3).map(|value| value.clone().into_boxed_str());
    metadata.volume = values
        .get(4)
        .cloned()
        .or_else(|| metadata.section.as_deref().and_then(default_volume))
        .map(String::into_boxed_str);
}

pub(super) fn title_lowercase(
    builder: &DocumentBuilder,
    title: NodeId,
) -> Option<(Box<str>, SourceSpan)> {
    let argument = builder
        .children(title)
        .and_then(|arguments| arguments.first())
        .copied()?;
    let title = builder.node_text(argument)?;
    let location = builder.node_location(argument)?;
    // `decode_visible_bytes` maps each malformed source byte to a Unicode
    // scalar. Its UTF-8 representation may take more than one byte, so a
    // string-byte index cannot be added to a raw source offset in that case.
    let offset = if builder.node_has_invalid_input_bytes(argument) {
        title
            .chars()
            .position(|character| character.is_ascii_lowercase())?
    } else {
        title.bytes().position(|byte| byte.is_ascii_lowercase())?
    };
    let offset = u32::try_from(offset).ok()?;
    // Expansion recovery can make the visible spelling wider than its
    // authored argument. Keep every public recovery location within the
    // argument's validated physical source range even in that degraded case.
    let start = location.start.saturating_add(offset).min(location.end);
    let end = start.saturating_add(1).min(location.end);
    let location = SourceSpan::new(location.source, start, end).ok()?;
    Some((title.to_owned().into_boxed_str(), location))
}

pub(super) fn title_unparseable_date(
    builder: &DocumentBuilder,
    title: NodeId,
) -> Option<(Box<str>, Option<SourceSpan>)> {
    let argument = title_date_argument(builder, title)?;
    let date = builder.node_text(argument)?;
    (!is_supported_title_date(date))
        .then(|| (date.to_owned().into(), builder.node_location(argument)))
}

pub(super) fn title_date_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.get(2).copied()
}

pub(super) fn title_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.first().copied()
}

pub(super) fn title_section_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.get(1).copied()
}

pub(super) fn title_argument_missing(builder: &DocumentBuilder, title: NodeId) -> bool {
    title_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_none_or(str::is_empty)
}

pub(super) fn title_section_missing(builder: &DocumentBuilder, title: NodeId) -> bool {
    title_section_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_none_or(str::is_empty)
}

pub(super) fn title_missing_date(builder: &DocumentBuilder, title: NodeId) -> bool {
    let explicit_empty_date = title_date_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_some_and(str::is_empty);
    explicit_empty_date || title_section_missing(builder, title)
}

/// Accept the stable man(7) date spellings that mandoc normalizes without a
/// recovery finding. Unknown author-supplied text remains public metadata,
/// but is reported through `TitleDateUnparseable`.
pub(super) fn is_supported_title_date(date: &str) -> bool {
    if date.is_empty() {
        return true;
    }
    let numeric = date.as_bytes();
    if numeric.len() == 10
        && numeric[4] == b'-'
        && numeric[7] == b'-'
        && numeric
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return true;
    }
    let Some((day, month, year)) = date.split_once('-').and_then(|(day, rest)| {
        let (month, year) = rest.split_once('-')?;
        Some((day, month, year))
    }) else {
        return matches!(date.split_whitespace().collect::<Vec<_>>().as_slice(), [month, day, year]
            if month_name(month)
                && day.strip_suffix(',').is_some_and(|day| day.parse::<u8>().is_ok())
                && year.len() == 4
                && year.bytes().all(|byte| byte.is_ascii_digit()));
    };
    day.parse::<u8>().is_ok()
        && month_name(month)
        && year.len() == 4
        && year.bytes().all(|byte| byte.is_ascii_digit())
}

/// Canonicalize the named month accepted by man(7)'s title-date grammar.
///
/// The owned AST retains the authored `.TH` argument, while document metadata
/// follows mandoc's stable long-month presentation. This keeps abbreviated
/// Sphinx dates such as `Jul 31, 2026` equivalent to `July 31, 2026` without
/// rewriting unsupported author text.
pub(super) fn normalize_title_date(date: &str) -> String {
    let mut fields = date.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_title_month) else {
        return date.to_owned();
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return date.to_owned();
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<u16>().ok()) else {
        return date.to_owned();
    };
    if fields.next().is_some() || day == 0 {
        return date.to_owned();
    }
    format!("{month} {day}, {year:04}")
}

pub(super) fn normalize_title_month(value: &str) -> Option<&'static str> {
    match value {
        "Jan" | "January" => Some("January"),
        "Feb" | "February" => Some("February"),
        "Mar" | "March" => Some("March"),
        "Apr" | "April" => Some("April"),
        "May" => Some("May"),
        "Jun" | "June" => Some("June"),
        "Jul" | "July" => Some("July"),
        "Aug" | "August" => Some("August"),
        "Sep" | "September" => Some("September"),
        "Oct" | "October" => Some("October"),
        "Nov" | "November" => Some("November"),
        "Dec" | "December" => Some("December"),
        _ => None,
    }
}

pub(super) fn month_name(value: &str) -> bool {
    matches!(
        value,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
            | "January"
            | "February"
            | "March"
            | "April"
            | "June"
            | "July"
            | "August"
            | "September"
            | "October"
            | "November"
            | "December"
    )
}

pub(super) fn default_volume(section: &str) -> Option<String> {
    let section = section.strip_suffix('p').unwrap_or(section);
    Some(
        match section {
            "1" => "General Commands Manual",
            "2" => "System Calls Manual",
            "3" => "Library Functions Manual",
            "4" => "Kernel Interfaces Manual",
            "5" => "File Formats Manual",
            "6" => "Games Manual",
            "7" => "Miscellaneous Information Manual",
            "8" => "System Manager's Manual",
            "9" => "Kernel Developer's Manual",
            _ => return None,
        }
        .to_owned(),
    )
}
