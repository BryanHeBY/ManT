//! Default Catppuccin Mocha colors and the reader's existing surface colors.

use ratatui::style::Color;

pub(crate) const CONTENT: Color = Color::Rgb(0, 0, 0);
pub(crate) const BASE: Color = Color::Rgb(30, 30, 46);
pub(crate) const MENU: Color = Color::Rgb(24, 24, 37);
pub(crate) const SIDEBAR: Color = Color::Rgb(17, 17, 27);
pub(crate) const SURFACE: Color = Color::Rgb(24, 24, 37);
pub(crate) const TLDR_SURFACE: Color = Color::Rgb(40, 36, 58);
pub(crate) const TLDR_NAV: Color = Color::Rgb(29, 26, 43);
pub(crate) const BORDER: Color = Color::Rgb(49, 50, 68);
pub(crate) const OVERLAY: Color = Color::Rgb(69, 71, 90);
pub(crate) const SCROLLBAR_TRACK: Color = BORDER;
pub(crate) const SCROLLBAR_THUMB: Color = OVERLAY;
pub(crate) const TEXT: Color = Color::Rgb(166, 173, 200);
pub(crate) const SUBTEXT: Color = Color::Rgb(127, 132, 156);
pub(crate) const SUBTEXT_BRIGHT: Color = Color::Rgb(186, 194, 222);
pub(crate) const STRONG: Color = Color::Rgb(205, 214, 244);
pub(crate) const SELECTED_TEXT: Color = Color::Rgb(245, 224, 220);
pub(crate) const HEADING: Color = Color::Rgb(148, 226, 213);
pub(crate) const LINK: Color = Color::Rgb(137, 220, 235);
pub(crate) const BLUE: Color = Color::Rgb(137, 180, 250);
pub(crate) const GREEN: Color = Color::Rgb(166, 227, 161);
pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175);
pub(crate) const PEACH: Color = Color::Rgb(250, 179, 135);
pub(crate) const MAUVE: Color = Color::Rgb(203, 166, 247);
pub(crate) const PINK: Color = Color::Rgb(245, 194, 231);
pub(crate) const MAROON: Color = Color::Rgb(235, 160, 172);
pub(crate) const LAVENDER: Color = Color::Rgb(180, 190, 254);
pub(crate) const SELECTED: Color = BORDER;
pub(crate) const TLDR_SELECTED: Color = Color::Rgb(73, 64, 95);
pub(crate) const SEARCH_MATCH: Color = OVERLAY;
pub(crate) const SEARCH_ACTIVE: Color = YELLOW;
