//! Shared terminal layout measurements for the Ratatui frontend.
//!
//! Keeping these values together makes the Rust UI's geometry explicit and
//! prevents individual widgets from drifting away from the established
//! `OpenTUI` layout during the migration.

use ratatui::layout::Margin;

pub(crate) const DEFAULT_SIDEBAR_WIDTH: u16 = 32;
pub(crate) const MIN_SIDEBAR_WIDTH: u16 = 24;
pub(crate) const MIN_CONTENT_WIDTH: u16 = 32;
pub(crate) const CONTENT_MARGIN: Margin = Margin {
    horizontal: 1,
    vertical: 1,
};
/// Blank column separating a full-width document surface from its scrollbar.
pub(crate) const CONTENT_SCROLLBAR_GAP: u16 = 1;

pub(crate) const fn maximum_sidebar_width(body_width: u16) -> u16 {
    let available = body_width.saturating_sub(MIN_CONTENT_WIDTH);
    if available < MIN_SIDEBAR_WIDTH {
        MIN_SIDEBAR_WIDTH
    } else {
        available
    }
}
