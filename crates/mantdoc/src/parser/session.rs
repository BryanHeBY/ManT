use super::{Diagnostic, DocumentBuilder, Environment, ParserConfig, Source, SourceResolver};
use crate::{SourceId, SourceName};

pub(super) struct ParseSession<'a, R: SourceResolver + ?Sized> {
    pub(super) config: &'a ParserConfig,
    pub(super) builder: &'a mut DocumentBuilder,
    pub(super) environment: &'a mut Environment,
    pub(super) active_sources: &'a mut Vec<SourceName>,
    pub(super) resolver: &'a mut R,
}

impl<'a, R: SourceResolver + ?Sized> ParseSession<'a, R> {
    pub(super) fn new(
        config: &'a ParserConfig,
        builder: &'a mut DocumentBuilder,
        environment: &'a mut Environment,
        active_sources: &'a mut Vec<SourceName>,
        resolver: &'a mut R,
    ) -> Self {
        Self {
            config,
            builder,
            environment,
            active_sources,
            resolver,
        }
    }
}

pub(super) struct ScanOutcome {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) deferred_post_validation_diagnostics: Vec<Diagnostic>,
    pub(super) source_bytes: usize,
    pub(super) source_files: usize,
    pub(super) text_bytes: usize,
    pub(super) expansion_steps: usize,
    pub(super) truncated: bool,
    pub(super) maximum_depth: usize,
    pub(super) previous_conditional: Option<bool>,
    pub(super) total_loop_iterations: usize,
    pub(super) saw_mdoc_operating_system: bool,
}

impl ScanOutcome {
    pub(super) fn root(source_bytes: usize, saw_mdoc_operating_system: bool) -> Self {
        Self {
            diagnostics: Vec::new(),
            deferred_post_validation_diagnostics: Vec::new(),
            source_bytes,
            source_files: 1,
            text_bytes: 0,
            expansion_steps: 0,
            truncated: false,
            maximum_depth: 1,
            previous_conditional: None,
            total_loop_iterations: 0,
            saw_mdoc_operating_system,
        }
    }
}

pub(super) struct SourceMachine<'source, 'session, 'context, R: SourceResolver + ?Sized> {
    pub(super) source: Source<'source>,
    pub(super) source_id: SourceId,
    pub(super) include_depth: usize,
    pub(super) session: &'session mut ParseSession<'context, R>,
    pub(super) outcome: ScanOutcome,
}

impl<'source, 'session, 'context, R: SourceResolver + ?Sized>
    SourceMachine<'source, 'session, 'context, R>
{
    pub(super) const fn new(
        source: Source<'source>,
        source_id: SourceId,
        include_depth: usize,
        session: &'session mut ParseSession<'context, R>,
        outcome: ScanOutcome,
    ) -> Self {
        Self {
            source,
            source_id,
            include_depth,
            session,
            outcome,
        }
    }
}
