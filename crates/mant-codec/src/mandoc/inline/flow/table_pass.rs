//! Tbl first-pass measurements over the shared native slot buffer.

use super::field_buffer::{CompactPass, compact_first_pass};
use super::native_slots::NativeSlots;
use super::{FontState, TabStops};
use crate::mandoc::inline::text_execution::record_native_event;
use crate::mandoc::roff_escape::{PresentationKind, RoffInlineEvent, decode};
use std::sync::Arc;

pub(super) fn first_pass(
    text: &str,
    target: usize,
    cell_width: usize,
    tabs: &Arc<TabStops>,
    font: &mut FontState,
) -> CompactPass {
    let events = decode(text);
    let mut scratch = NativeSlots::default();
    let mut writes = Vec::new();
    for event in &events {
        match event {
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                scratch.motion(argument.as_deref());
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => scratch.line(argument, cell_width, font.current),
            RoffInlineEvent::Overstrike { source, .. } => {
                scratch.overstrike(source, font.current, |_| None);
            }
            RoffInlineEvent::Font(selection) => font.select(*selection),
            RoffInlineEvent::PreviousFont => font.restore(),
            _ => {
                record_native_event(event, &mut writes, false);
                for write in writes.drain(..) {
                    scratch.native_write(write, font.current, None);
                }
            }
        }
    }
    compact_first_pass(
        scratch
            .into_fill_slots()
            .into_iter()
            .map(|slot| (slot.cell, slot.count)),
        target,
        tabs,
    )
}
