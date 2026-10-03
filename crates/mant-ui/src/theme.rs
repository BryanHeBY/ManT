//! Role-based styles shared by outline and document rendering.
//!
//! The hardcoded default theme separates semantic roles, source composition
//! and interaction overlays. Future configuration can replace style values
//! without changing content traversal or navigation semantics.

use mant_ir::EntryKind;
use ratatui::style::Style;

use crate::NavKind;

mod defaults;
mod palette;
mod roles;
pub(crate) use palette::*;
pub(crate) use roles::{InteractionRole, StyleRole};

#[cfg(test)]
mod tests;

/// Fixed role slots avoid allocation and map lookups in per-span rendering.
#[derive(Clone, Debug)]
pub(crate) struct Theme {
    styles: [Style; StyleRole::COUNT],
    interactions: [Style; InteractionRole::COUNT],
}

impl Theme {
    const fn empty() -> Self {
        Self {
            styles: [Style::new(); StyleRole::COUNT],
            interactions: [Style::new(); InteractionRole::COUNT],
        }
    }

    pub(crate) const fn set_style(&mut self, role: StyleRole, style: Style) {
        self.styles[role as usize] = style;
    }

    pub(crate) const fn set_interaction(&mut self, role: InteractionRole, style: Style) {
        self.interactions[role as usize] = style;
    }

    pub(crate) const fn style(&self, role: StyleRole) -> Style {
        self.styles[role as usize]
    }

    pub(crate) fn interact(&self, base: Style, role: InteractionRole) -> Style {
        base.patch(self.interactions[role as usize])
    }

    /// Authored markup layers over the base; validated name roles come last.
    pub(crate) fn inline_style(
        &self,
        mut base: Style,
        source: mant_render::InlinePresentation,
    ) -> Style {
        for (enabled, role) in [
            (source.strong, StyleRole::Strong),
            (source.emphasis, StyleRole::Emphasis),
            (source.code, StyleRole::InlineCode),
            (source.link, StyleRole::Link),
        ] {
            if enabled {
                base = base.patch(self.style(role));
            }
        }
        if let Some(kind) = source.entry_kind {
            base = base.patch(self.style(StyleRole::for_entry(kind)));
        }
        base
    }

    pub(crate) fn navigation_style(&self, kind: NavKind, selected: bool) -> Style {
        let (role, surface) = match kind {
            NavKind::Root => (StyleRole::OutlineRoot, StyleRole::OutlineSurface),
            NavKind::Section => (StyleRole::OutlineSection, StyleRole::OutlineSurface),
            NavKind::EntryGroup => (StyleRole::OutlineEntries, StyleRole::OutlineSurface),
            NavKind::ReferenceGroup => (StyleRole::OutlineReferences, StyleRole::OutlineSurface),
            NavKind::Reference => (StyleRole::OutlineReference, StyleRole::OutlineSurface),
            NavKind::ReferenceNotice => (StyleRole::Notice, StyleRole::OutlineSurface),
            NavKind::Entry(EntryKind::Term) => (StyleRole::OutlineTerm, StyleRole::OutlineSurface),
            NavKind::Entry(kind) => (StyleRole::for_entry(kind), StyleRole::OutlineSurface),
            NavKind::Tldr => (StyleRole::TldrTitle, StyleRole::TldrOutlineSurface),
        };
        let style = self.style(surface).patch(self.style(role));
        self.navigation_overlay(style, kind, selected)
    }

    pub(crate) fn navigation_reference_style(&self, kind: NavKind, selected: bool) -> Style {
        let style = self
            .navigation_style(kind, false)
            .patch(self.style(StyleRole::Link));
        self.navigation_overlay(style, kind, selected)
    }

    fn navigation_overlay(&self, style: Style, kind: NavKind, selected: bool) -> Style {
        if selected {
            self.interact(
                style,
                if kind == NavKind::Tldr {
                    InteractionRole::TldrOutlineSelection
                } else {
                    InteractionRole::OutlineSelection
                },
            )
        } else {
            style
        }
    }
}

pub(crate) static DEFAULT_THEME: Theme = defaults::dark();

pub(crate) const fn style(role: StyleRole) -> Style {
    DEFAULT_THEME.style(role)
}

pub(crate) fn interact(base: Style, role: InteractionRole) -> Style {
    DEFAULT_THEME.interact(base, role)
}

pub(crate) fn inline_style(base: Style, source: mant_render::InlinePresentation) -> Style {
    DEFAULT_THEME.inline_style(base, source)
}

pub(crate) fn navigation_style(kind: NavKind, selected: bool) -> Style {
    DEFAULT_THEME.navigation_style(kind, selected)
}

pub(crate) fn navigation_reference_style(kind: NavKind, selected: bool) -> Style {
    DEFAULT_THEME.navigation_reference_style(kind, selected)
}

#[cfg(test)]
pub(crate) fn entry_color(kind: EntryKind) -> ratatui::style::Color {
    style(StyleRole::for_entry(kind))
        .fg
        .expect("default entry color")
}
