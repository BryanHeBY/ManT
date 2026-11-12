use crate::presentation;
use std::cell::Cell;

use mant_ir::ResolvedContent;

use mant_sources::{
    DocumentSourcesPrune, DocumentSourcesPruneSchema, DocumentSourcesUpdate,
    DocumentSourcesUpdateSchema,
};

use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, EntryFacts, EntryKind, Inline,
    LayoutHint, NameCase, Section, SourceFormat, TldrDocument, TldrOrigin,
};
use mant_protocol::{
    CatalogSchema, DoctorCheck, DoctorCheckStatus, DoctorEnvironment, DoctorReport,
    DocumentSummary, MarkdownOrigin, Producer, QueryInput, QueryRequest, TldrCacheAction,
    TldrCacheUpdate,
};

use crate::{
    CLI_PROTOCOL_VERSION,
    application::request_for_address,
    arguments::{self, ColorMode, Command, DisplayMode, OutputOptions},
    cli::{run_command, run_with_host},
    error::Failure,
    host::CliHost,
    output_policy::{TerminalCapabilities, TerminalKind, resolve_process_presentation},
    request_input::read_native_request,
};
use mant_loader::LoadPolicy;
use mant_protocol::{CatalogQuery, DocumentAddress, DocumentCatalog};

#[cfg(feature = "tui")]
mod navigation;

mod support;
use support::*;

mod commands;

mod output;

mod protocol;

mod queries;

mod terminal;
