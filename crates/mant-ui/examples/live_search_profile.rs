//! Explicit-input live-search scheduling probe; no host corpus dependency in CI.
//! Run: `cargo run -p mant-ui --example live_search_profile -- FILE QUERY`

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, ReaderOptions};
use ratatui::{Terminal, backend::TestBackend};
use std::{
    error::Error,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: live_search_profile FILE QUERY".into());
    }
    let path = Path::new(&args[0]);
    let document = if path.extension().is_some_and(|extension| extension == "md") {
        mant_codec::parse_markdown(&std::fs::read_to_string(path)?, None)?.document
    } else {
        mant_loader::parse_manual_source(path)?
    };
    let bundle = Arc::new(ResolvedContent {
        address: None,
        label: args[0].clone(),
        document: Some(document),
        tldr: None,
    });
    let start = Instant::now();
    let mut app = App::from_shared(ReaderOptions::new(bundle));
    let mut terminal = Terminal::new(TestBackend::new(120, 30))?;
    terminal.draw(|frame| app.draw(frame))?;
    println!(
        "prepare_and_first_frame_ms\t{:.3}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let mut typing_max = Duration::ZERO;
    for character in args[1].chars() {
        let start = Instant::now();
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        typing_max = typing_max.max(start.elapsed());
    }
    let mut now = Instant::now();
    let mut slices = 0;
    let mut draws = 0;
    let mut slice_max = Duration::ZERO;
    let mut frame_max = Duration::ZERO;
    let mut cpu = Duration::ZERO;
    // Advance the host clock instead of sleeping through debounce/slice delays.
    while let Some(delay) = app.next_wakeup(now) {
        if slices == 1_000_000 {
            return Err("search exceeded probe work ceiling".into());
        }
        now += delay;
        let start = Instant::now();
        let redraw = app.tick(now).needs_redraw();
        let elapsed = start.elapsed();
        slice_max = slice_max.max(elapsed);
        cpu += elapsed;
        slices += 1;
        if redraw {
            let start = Instant::now();
            terminal.draw(|frame| app.draw(frame))?;
            frame_max = frame_max.max(start.elapsed());
            draws += 1;
        }
    }
    terminal.draw(|frame| app.draw(frame))?;
    let status = (0..120)
        .map(|column| terminal.backend().buffer()[(column, 29)].symbol())
        .collect::<String>();
    println!(
        "typing_max_ms\t{:.3}\nscan_slices\t{slices}\nscan_cpu_ms\t{:.3}\nslice_max_ms\t{:.3}\nsearch_frames\t{draws}\nframe_max_ms\t{:.3}\nstatus\t{}",
        typing_max.as_secs_f64() * 1000.0,
        cpu.as_secs_f64() * 1000.0,
        slice_max.as_secs_f64() * 1000.0,
        frame_max.as_secs_f64() * 1000.0,
        status.trim()
    );
    Ok(())
}
