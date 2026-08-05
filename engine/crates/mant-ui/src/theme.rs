//! Central color palette shared by the Ratatui widgets and document renderer.

use ratatui::style::Color;

// Catppuccin Mocha values used by the original OpenTUI frontend. Keeping the
// palette semantic makes later widgets share one visual language instead of
// accumulating one-off RGB literals.
pub const CONTENT: Color = Color::Rgb(0, 0, 0);
pub const BASE: Color = Color::Rgb(30, 30, 46);
pub const MENU: Color = Color::Rgb(24, 24, 37);
pub const SIDEBAR: Color = Color::Rgb(17, 17, 27);
pub const SURFACE: Color = Color::Rgb(24, 24, 37);
pub const TLDR_SURFACE: Color = Color::Rgb(40, 36, 58);
pub const TLDR_NAV: Color = Color::Rgb(29, 26, 43);
pub const BORDER: Color = Color::Rgb(49, 50, 68);
pub const OVERLAY: Color = Color::Rgb(69, 71, 90);
pub const TEXT: Color = Color::Rgb(166, 173, 200);
pub const SUBTEXT: Color = Color::Rgb(127, 132, 156);
pub const SUBTEXT_BRIGHT: Color = Color::Rgb(186, 194, 222);
pub const SELECTED_TEXT: Color = Color::Rgb(245, 224, 220);
pub const HEADING: Color = Color::Rgb(148, 226, 213);
pub const LINK: Color = Color::Rgb(137, 220, 235);
pub const BLUE: Color = Color::Rgb(137, 180, 250);
pub const GREEN: Color = Color::Rgb(166, 227, 161);
pub const YELLOW: Color = Color::Rgb(249, 226, 175);
pub const PEACH: Color = Color::Rgb(250, 179, 135);
pub const MAUVE: Color = Color::Rgb(203, 166, 247);
pub const SELECTED: Color = BORDER;
pub const TLDR_SELECTED: Color = Color::Rgb(73, 64, 95);
pub const SEARCH_MATCH: Color = Color::Rgb(69, 71, 90);
pub const SEARCH_ACTIVE: Color = YELLOW;
