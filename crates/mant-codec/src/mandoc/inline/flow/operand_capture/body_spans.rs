//! Accepted native BODY ranges remain separate from the words they contain.
use std::{collections::HashMap, ops::Range};

#[derive(Clone, Default)]
pub(super) struct AcceptedBodies {
    word_bodies: HashMap<u64, u32>,
    runs: Vec<ScopeRun>,
}

#[derive(Clone)]
struct ScopeRun {
    body: u32,
    bytes: Range<usize>,
}

impl AcceptedBodies {
    pub(super) fn record(&mut self, serial: u64, body: u32) {
        self.word_bodies.insert(serial, body);
    }

    pub(super) fn observe(&mut self, bytes: Range<usize>, serial: u64, text: &str) {
        let Some(&body) = self.word_bodies.get(&serial) else {
            return;
        };
        if let Some(last) = self.runs.last_mut()
            && last.body == body
            && text[last.bytes.end..bytes.start].trim().is_empty()
        {
            last.bytes.end = bytes.end;
        } else {
            self.runs.push(ScopeRun { body, bytes });
        }
    }

    pub(super) fn take(&mut self) -> Vec<Range<usize>> {
        self.runs.drain(..).map(|run| run.bytes).collect()
    }
}
