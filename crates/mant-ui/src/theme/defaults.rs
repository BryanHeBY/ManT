//! Hardcoded default values; role resolution and overlay order live elsewhere.

use ratatui::style::{Modifier, Style};

use super::palette::{
    BASE, BLUE, CONTENT, GREEN, HEADING, LAVENDER, LINK, MAROON, MAUVE, OVERLAY, PEACH, PINK,
    SEARCH_ACTIVE, SEARCH_MATCH, SELECTED, SELECTED_TEXT, SIDEBAR, STRONG, SUBTEXT, SUBTEXT_BRIGHT,
    SURFACE, TEXT, TLDR_NAV, TLDR_SELECTED, TLDR_SURFACE, YELLOW,
};
use super::{InteractionRole as Interaction, StyleRole as Role, Theme};

pub(super) const fn dark() -> Theme {
    let mut theme = Theme::empty();
    text(&mut theme);
    entries(&mut theme);
    outline(&mut theme);
    code(&mut theme);
    surfaces(&mut theme);
    interactions(&mut theme);
    theme
}

const fn text(theme: &mut Theme) {
    theme.set_style(Role::Text, Style::new().fg(TEXT));
    theme.set_style(
        Role::Heading,
        Style::new().fg(HEADING).add_modifier(Modifier::BOLD),
    );
    theme.set_style(
        Role::Strong,
        Style::new().fg(STRONG).add_modifier(Modifier::BOLD),
    );
    theme.set_style(Role::Emphasis, Style::new().add_modifier(Modifier::ITALIC));
    theme.set_style(
        Role::InlineCode,
        Style::new().fg(SUBTEXT_BRIGHT).bg(SURFACE),
    );
    theme.set_style(
        Role::Link,
        Style::new().fg(LINK).add_modifier(Modifier::UNDERLINED),
    );
    theme.set_style(Role::ListMarker, Style::new().fg(SUBTEXT_BRIGHT));
    theme.set_style(Role::Equation, Style::new().fg(YELLOW));
    theme.set_style(Role::Unsupported, Style::new().fg(PEACH));
    theme.set_style(Role::Metadata, Style::new().fg(SUBTEXT));
    theme.set_style(Role::Notice, Style::new().fg(YELLOW));
    theme.set_style(
        Role::TldrTitle,
        Style::new().fg(MAUVE).add_modifier(Modifier::BOLD),
    );
    theme.set_style(Role::TldrExample, Style::new().fg(GREEN));
    theme.set_style(Role::TldrCommand, Style::new().fg(PEACH));
    theme.set_style(Role::TldrFrame, Style::new().fg(MAUVE));
    theme.set_style(Role::Rule, Style::new().fg(OVERLAY));
}

const fn entries(theme: &mut Theme) {
    theme.set_style(Role::Parameter, Style::new().fg(GREEN));
    theme.set_style(Role::Command, Style::new().fg(PEACH));
    theme.set_style(Role::Environment, Style::new().fg(MAUVE));
    theme.set_style(Role::Configuration, Style::new().fg(YELLOW));
    theme.set_style(Role::Variable, Style::new().fg(PINK));
    theme.set_style(Role::Value, Style::new().fg(BLUE));
    theme.set_style(Role::Term, Style::new().fg(TEXT));
}

const fn outline(theme: &mut Theme) {
    theme.set_style(Role::OutlineRoot, Style::new().fg(SUBTEXT_BRIGHT));
    theme.set_style(
        Role::OutlineSection,
        Style::new().fg(BLUE).add_modifier(Modifier::BOLD),
    );
    theme.set_style(
        Role::OutlineEntries,
        Style::new().fg(MAROON).add_modifier(Modifier::BOLD),
    );
    theme.set_style(
        Role::OutlineReferences,
        Style::new().fg(LAVENDER).add_modifier(Modifier::BOLD),
    );
    theme.set_style(
        Role::OutlineReference,
        Style::new().fg(LINK).add_modifier(Modifier::UNDERLINED),
    );
    theme.set_style(Role::OutlineTerm, Style::new().fg(STRONG));
    theme.set_style(Role::TreeGuide, Style::new().fg(OVERLAY));
    theme.set_style(Role::TreeFocus, Style::new().fg(PEACH));
    theme.set_style(Role::TreeContinuationFocus, Style::new().fg(PINK));
}

const fn code(theme: &mut Theme) {
    theme.set_style(
        Role::CodeComment,
        Style::new().fg(SUBTEXT).add_modifier(Modifier::ITALIC),
    );
    theme.set_style(Role::CodeString, Style::new().fg(BLUE));
    theme.set_style(Role::CodeOption, Style::new().fg(HEADING));
    theme.set_style(Role::CodeNumber, Style::new().fg(YELLOW));
    theme.set_style(
        Role::CodeKeyword,
        Style::new().fg(MAUVE).add_modifier(Modifier::BOLD),
    );
}

const fn surfaces(theme: &mut Theme) {
    theme.set_style(Role::ContentSurface, Style::new().bg(CONTENT));
    theme.set_style(Role::OutlineSurface, Style::new().bg(SIDEBAR));
    theme.set_style(Role::CodeSurface, Style::new().bg(SURFACE));
    theme.set_style(Role::TldrSurface, Style::new().bg(TLDR_SURFACE));
    theme.set_style(Role::TldrOutlineSurface, Style::new().bg(TLDR_NAV));
}

const fn interactions(theme: &mut Theme) {
    theme.set_interaction(
        Interaction::Selection,
        Style::new().fg(SELECTED_TEXT).bg(SELECTED),
    );
    theme.set_interaction(
        Interaction::OutlineSelection,
        Style::new()
            .fg(SELECTED_TEXT)
            .bg(SELECTED)
            .add_modifier(Modifier::BOLD),
    );
    theme.set_interaction(
        Interaction::TldrOutlineSelection,
        Style::new()
            .fg(MAUVE)
            .bg(TLDR_SELECTED)
            .add_modifier(Modifier::BOLD),
    );
    theme.set_interaction(Interaction::SearchMatch, Style::new().bg(SEARCH_MATCH));
    theme.set_interaction(
        Interaction::SearchActive,
        Style::new()
            .fg(BASE)
            .bg(SEARCH_ACTIVE)
            .add_modifier(Modifier::BOLD),
    );
}
