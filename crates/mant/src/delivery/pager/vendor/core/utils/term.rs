//! Cursor drawing helpers. ManT's host owns terminal acquisition and cleanup.

#![allow(dead_code)]

use crate::delivery::pager::native::error::MinusError;
use crossterm::{
    cursor, queue,
    terminal::{self, Clear},
};
use std::io;

/// Moves the terminal cursor to given x, y coordinates
///
/// The `flush` parameter will immediately flush the buffer if it is set to `true`
pub fn move_cursor(
    out: &mut impl io::Write,
    x: u16,
    y: u16,
    flush: bool,
) -> Result<(), MinusError> {
    queue!(out, cursor::MoveTo(x, y))?;
    if flush {
        out.flush()?;
    }
    Ok(())
}

pub fn clear_entire_screen(out: &mut impl io::Write, flush: bool) -> crate::delivery::pager::native::Result {
    queue!(out, Clear(terminal::ClearType::All))?;
    if flush {
        out.flush()?;
    }
    Ok(())
}
