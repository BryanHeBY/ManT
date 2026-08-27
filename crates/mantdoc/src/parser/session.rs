use super::{
    BranchOutcome, Diagnostic, DocumentBuilder, Environment, ParserConfig, Source, SourceResolver,
};
use crate::{SourceId, SourceName};

pub(super) struct ParserCore<'a, R: SourceResolver + ?Sized> {
    pub(super) config: &'a ParserConfig,
    pub(super) builder: &'a mut DocumentBuilder,
    pub(super) environment: &'a mut Environment,
    pub(super) active_sources: &'a mut Vec<SourceName>,
    pub(super) resolver: &'a mut R,
}

impl<'a, R: SourceResolver + ?Sized> ParserCore<'a, R> {
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

pub(super) struct ParseState {
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) deferred_post_validation_diagnostics: Vec<Diagnostic>,
    pub(super) source_bytes: usize,
    pub(super) source_files: usize,
    pub(super) text_bytes: usize,
    pub(super) expansion_steps: usize,
    pub(super) truncated: bool,
    pub(super) maximum_depth: usize,
    pub(super) previous_conditional: Option<BranchOutcome>,
    pub(super) total_loop_iterations: usize,
    pub(super) saw_mdoc_operating_system: bool,
}

impl ParseState {
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

pub(super) struct SourceFrame<'source, 'core, 'context, R: SourceResolver + ?Sized> {
    pub(super) source: Source<'source>,
    pub(super) source_id: SourceId,
    pub(super) include_depth: usize,
    pub(super) core: &'core mut ParserCore<'context, R>,
    pub(super) outcome: ParseState,
}

impl<'source, 'core, 'context, R: SourceResolver + ?Sized>
    SourceFrame<'source, 'core, 'context, R>
{
    pub(super) const fn new(
        source: Source<'source>,
        source_id: SourceId,
        include_depth: usize,
        core: &'core mut ParserCore<'context, R>,
        outcome: ParseState,
    ) -> Self {
        Self {
            source,
            source_id,
            include_depth,
            core,
            outcome,
        }
    }
}
