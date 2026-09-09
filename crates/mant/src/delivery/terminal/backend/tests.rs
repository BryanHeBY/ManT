use super::*;
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    layout::Rect,
    style::{Color, Modifier},
    widgets::Paragraph,
};

use TerminalResource::{AlternateScreen, HiddenCursor, MouseCapture, RawMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Acquire(TerminalResource),
    Release(TerminalResource),
}

#[derive(Default)]
struct State {
    trace: Vec<Event>,
    active: Vec<TerminalResource>,
    acquire_failure: Option<TerminalResource>,
    release_failure: Option<TerminalResource>,
}

struct Operations(Rc<RefCell<State>>);

impl TerminalOps for Operations {
    fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.trace.push(Event::Acquire(resource));
        state.active.push(resource);
        if state.acquire_failure == Some(resource) {
            Err(io::Error::other("partially applied acquisition"))
        } else {
            Ok(())
        }
    }

    fn release(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.trace.push(Event::Release(resource));
        if state.release_failure == Some(resource) {
            state.release_failure = None;
            Err(io::Error::other("injected release failure"))
        } else {
            state.active.retain(|active| *active != resource);
            Ok(())
        }
    }
}

fn lease(state: &Rc<RefCell<State>>) -> SharedLease<Operations> {
    Rc::new(RefCell::new(TerminalLease::new(Operations(Rc::clone(
        state,
    )))))
}

fn fixed_terminal(lease: &SharedLease<Operations>) -> Terminal<LeasedBackend<Vec<u8>, Operations>> {
    Terminal::with_options(
        LeasedBackend::new(Vec::new(), Rc::clone(lease)),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 20, 4)),
        },
    )
    .unwrap()
}

#[test]
fn actual_tui_setup_order_and_every_partial_failure_are_owned() {
    let order = [RawMode, AlternateScreen, MouseCapture];
    for failed in 0..order.len() {
        let state = Rc::new(RefCell::new(State {
            acquire_failure: Some(order[failed]),
            ..State::default()
        }));
        let lease = lease(&state);
        assert!(acquire_tui(&lease).is_err());
        drop(lease);
        let expected: Vec<_> = order[..=failed]
            .iter()
            .copied()
            .map(Event::Acquire)
            .chain(order[..=failed].iter().copied().rev().map(Event::Release))
            .collect();
        assert_eq!(state.borrow().trace, expected);
        assert!(state.borrow().active.is_empty());
    }
}

#[test]
fn ratatui_cursor_changes_and_drop_share_the_session_ledger() {
    let state = Rc::new(RefCell::new(State::default()));
    let lease = lease(&state);
    acquire_tui(&lease).unwrap();
    let mut terminal = fixed_terminal(&lease);
    for _ in 0..2 {
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("same"), frame.area()))
            .unwrap();
    }
    // No repeated physical Hide merely because another frame was drawn.
    assert_eq!(
        state.borrow().trace,
        [
            Event::Acquire(RawMode),
            Event::Acquire(AlternateScreen),
            Event::Acquire(MouseCapture),
            Event::Acquire(HiddenCursor)
        ]
    );
    terminal
        .draw(|frame| frame.set_cursor_position((1, 1)))
        .unwrap();
    terminal.draw(|_| {}).unwrap();
    restore(&lease).unwrap();
    let before_drop = state.borrow().trace.clone();
    drop(terminal);
    drop(lease);
    assert_eq!(
        state.borrow().trace,
        before_drop,
        "Ratatui and lease Drop must not restore twice"
    );
    assert_eq!(
        &before_drop[4..],
        [
            Event::Release(HiddenCursor),
            Event::Acquire(HiddenCursor),
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(AlternateScreen),
            Event::Release(RawMode)
        ]
    );
    assert!(state.borrow().active.is_empty());
}

#[test]
fn a_partial_hide_failure_is_recovered_even_when_ratatui_did_not_record_it() {
    let state = Rc::new(RefCell::new(State {
        acquire_failure: Some(HiddenCursor),
        ..State::default()
    }));
    let lease = lease(&state);
    acquire_tui(&lease).unwrap();
    let mut terminal = fixed_terminal(&lease);
    assert!(terminal.draw(|_| {}).is_err());
    drop(terminal);
    assert!(lease.borrow().has_pending(HiddenCursor));
    restore(&lease).unwrap();
    assert!(state.borrow().active.is_empty());
    assert_eq!(
        &state.borrow().trace[4..],
        [
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(AlternateScreen),
            Event::Release(RawMode)
        ]
    );
}

#[test]
fn failed_cursor_release_is_retried_by_ratatui_without_releasing_other_modes_twice() {
    let state = Rc::new(RefCell::new(State {
        release_failure: Some(HiddenCursor),
        ..State::default()
    }));
    let lease = lease(&state);
    acquire_tui(&lease).unwrap();
    let mut terminal = fixed_terminal(&lease);
    terminal.draw(|_| {}).unwrap();
    assert_eq!(
        restore(&lease).unwrap_err().to_string(),
        "injected release failure"
    );
    assert_eq!(state.borrow().active, [HiddenCursor]);
    drop(terminal);
    drop(lease);
    assert_eq!(
        &state.borrow().trace[4..],
        [
            Event::Release(HiddenCursor),
            Event::Release(MouseCapture),
            Event::Release(AlternateScreen),
            Event::Release(RawMode),
            Event::Release(HiddenCursor)
        ]
    );
    assert!(state.borrow().active.is_empty());
}

#[test]
fn explicit_show_failure_remains_pending_for_later_session_cleanup() {
    let state = Rc::new(RefCell::new(State {
        release_failure: Some(HiddenCursor),
        ..State::default()
    }));
    let lease = lease(&state);
    acquire_tui(&lease).unwrap();
    let mut terminal = fixed_terminal(&lease);
    terminal.draw(|_| {}).unwrap();
    assert!(terminal.show_cursor().is_err());
    assert!(lease.borrow().has_pending(HiddenCursor));
    restore(&lease).unwrap();
    let cleaned = state.borrow().trace.clone();
    drop(terminal);
    drop(lease);
    assert_eq!(state.borrow().trace, cleaned);
    assert_eq!(
        cleaned
            .iter()
            .filter(|event| **event == Event::Release(HiddenCursor))
            .count(),
        2
    );
    assert!(state.borrow().active.is_empty());
}

#[test]
fn reentrant_ratatui_drop_returns_an_error_instead_of_panicking() {
    let state = Rc::new(RefCell::new(State::default()));
    let lease = lease(&state);
    acquire_tui(&lease).unwrap();
    let mut terminal = fixed_terminal(&lease);
    terminal.draw(|_| {}).unwrap();
    let busy = lease.borrow_mut();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(terminal))).is_ok());
    drop(busy);
    restore(&lease).unwrap();
    assert!(state.borrow().active.is_empty());
}

#[test]
fn drawing_and_non_mode_commands_delegate_byte_for_byte_to_crossterm() {
    let state = Rc::new(RefCell::new(State::default()));
    let lease = lease(&state);
    let mut plain = Vec::new();
    let mut wrapped = Vec::new();
    let mut cell = Cell::default();
    cell.set_symbol("界")
        .set_fg(Color::Green)
        .set_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
    let commands = |backend: &mut CrosstermBackend<&mut Vec<u8>>| {
        backend.draw([(2, 1, &cell)].into_iter()).unwrap();
        backend.append_lines(1).unwrap();
        backend.set_cursor_position((3, 2)).unwrap();
        backend.clear_region(ClearType::UntilNewLine).unwrap();
        Backend::flush(backend).unwrap();
    };
    commands(&mut CrosstermBackend::new(&mut plain));
    {
        let mut backend = LeasedBackend::new(&mut wrapped, Rc::clone(&lease));
        backend.draw([(2, 1, &cell)].into_iter()).unwrap();
        backend.append_lines(1).unwrap();
        backend.set_cursor_position((3, 2)).unwrap();
        backend.clear_region(ClearType::UntilNewLine).unwrap();
        Backend::flush(&mut backend).unwrap();
    }
    assert_eq!(wrapped, plain);
    assert!(!wrapped.is_empty());
    assert!(state.borrow().trace.is_empty());
}
