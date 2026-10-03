//! Theme customization changes presentation without changing semantic routing.

use mant_ir::{EntryKind, ParameterKind};
use ratatui::style::{Color, Modifier, Style};

use super::*;

#[test]
fn every_default_role_has_a_style_and_structural_roles_are_distinct() {
    assert!(
        DEFAULT_THEME
            .styles
            .iter()
            .all(|style| *style != Style::new())
    );
    assert!(
        DEFAULT_THEME
            .interactions
            .iter()
            .all(|style| *style != Style::new())
    );
    let section = navigation_style(NavKind::Section, false);
    let group = navigation_style(NavKind::ReferenceGroup, false);
    let value = navigation_style(NavKind::Entry(EntryKind::Value), false);
    assert_eq!(section.fg, Some(BLUE));
    assert!(section.add_modifier.contains(Modifier::BOLD));
    assert_eq!(group.fg, Some(LAVENDER));
    assert_eq!(value.fg, Some(BLUE));
    assert_ne!(section.fg, group.fg);
    assert_ne!(section.add_modifier, value.add_modifier);
    assert_eq!(style(StyleRole::Heading).fg, Some(HEADING));
    let entries = navigation_style(NavKind::EntryGroup, false);
    assert_eq!(entries.fg, Some(MAROON));
    assert_ne!(entries.fg, group.fg);
    assert!(entries.add_modifier.contains(Modifier::BOLD));
    assert!(group.add_modifier.contains(Modifier::BOLD));
    assert_eq!(style(StyleRole::InlineCode).fg, Some(SUBTEXT_BRIGHT));
    assert_eq!(style(StyleRole::InlineCode).bg, Some(SURFACE));
}

#[test]
fn entry_families_keep_their_semantic_colors() {
    assert_eq!(entry_color(EntryKind::Term), TEXT);
    assert_eq!(entry_color(EntryKind::Value), BLUE);
    assert_ne!(entry_color(EntryKind::Term), SUBTEXT);
    assert_eq!(entry_color(EntryKind::ConfigurationKey), YELLOW);
    assert_eq!(entry_color(EntryKind::EnvironmentVariable), MAUVE);
    assert_eq!(entry_color(EntryKind::Variable), PINK);
    assert_ne!(entry_color(EntryKind::EnvironmentVariable), LINK);
    assert_ne!(entry_color(EntryKind::EnvironmentVariable), HEADING);
    for parameter_kind in [
        ParameterKind::Option,
        ParameterKind::Marker,
        ParameterKind::Operand,
    ] {
        assert_eq!(entry_color(EntryKind::Parameter { parameter_kind }), GREEN);
    }
}

#[test]
fn outline_section_style_can_change_independently_of_body_heading_style() {
    let mut theme = DEFAULT_THEME.clone();
    let section = Style::new().fg(Color::Green).add_modifier(Modifier::ITALIC);
    theme.set_style(StyleRole::OutlineSection, section);
    assert_eq!(
        theme.navigation_style(NavKind::Section, false),
        theme.style(StyleRole::OutlineSurface).patch(section)
    );
    assert_eq!(
        theme.style(StyleRole::Heading),
        DEFAULT_THEME.style(StyleRole::Heading)
    );
    assert_eq!(
        DEFAULT_THEME.style(StyleRole::OutlineSection).fg,
        Some(BLUE)
    );
}

#[test]
fn entry_and_reference_group_styles_are_independently_customizable() {
    let mut theme = DEFAULT_THEME.clone();
    let entries = Style::new().fg(Color::Red).add_modifier(Modifier::ITALIC);
    theme.set_style(StyleRole::OutlineEntries, entries);
    assert_eq!(
        theme.navigation_style(NavKind::EntryGroup, false),
        theme.style(StyleRole::OutlineSurface).patch(entries)
    );
    assert_eq!(
        theme.navigation_style(NavKind::ReferenceGroup, false),
        DEFAULT_THEME.navigation_style(NavKind::ReferenceGroup, false)
    );
}

#[test]
fn a_custom_entry_role_reaches_outline_and_body_with_inherited_affordances() {
    let mut theme = DEFAULT_THEME.clone();
    let color = Color::Rgb(170, 120, 210);
    theme.set_style(StyleRole::Command, Style::new().fg(color));
    assert_eq!(
        theme
            .navigation_style(NavKind::Entry(EntryKind::Command), false)
            .fg,
        Some(color)
    );
    let source = mant_render::InlinePresentation {
        strong: true,
        emphasis: true,
        code: true,
        link: true,
        entry_kind: Some(EntryKind::Command),
        ..Default::default()
    };
    let body = theme.inline_style(theme.style(StyleRole::Text), source);
    assert_eq!(body.fg, Some(color));
    assert_eq!(body.bg, Some(SURFACE));
    assert!(
        body.add_modifier
            .contains(Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED)
    );
    assert_eq!(
        theme.style(StyleRole::OutlineSection),
        DEFAULT_THEME.style(StyleRole::OutlineSection)
    );
}

#[test]
fn custom_styles_and_interactions_compose_without_rewriting_base_roles() {
    let mut theme = DEFAULT_THEME.clone();
    let code = Style::new()
        .fg(Color::White)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let active = Style::new().bg(Color::Red).add_modifier(Modifier::REVERSED);
    theme.set_style(StyleRole::InlineCode, code);
    theme.set_interaction(InteractionRole::SearchActive, active);
    let base = theme.inline_style(
        Style::new().add_modifier(Modifier::ITALIC),
        mant_render::InlinePresentation {
            code: true,
            link: true,
            ..Default::default()
        },
    );
    let highlighted = theme.interact(base, InteractionRole::SearchActive);
    assert_eq!(highlighted.fg, Some(LINK));
    assert_eq!(highlighted.bg, Some(Color::Red));
    assert!(
        highlighted.add_modifier.contains(
            Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED
        )
    );
    assert_eq!(theme.style(StyleRole::InlineCode), code);
    assert_eq!(DEFAULT_THEME.style(StyleRole::InlineCode).bg, Some(SURFACE));
}
