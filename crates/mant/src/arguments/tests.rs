use mant_ir::{EntryKind, ParameterKind};
#[cfg(feature = "roff")]
use mant_protocol::CatalogDocumentKind;
use mant_protocol::{
    CatalogQuery, DocumentScope, DocumentSelector, DocumentTraversal, EntryProjection, InputFormat,
    QueryInput, QueryRequest, QueryView, RequestSchema, ScopeQueryView, SearchCase, SearchScope,
    SearchSyntax,
};

use super::{
    ColorMode, Command, DisplayMode, LoadPolicy, OutputOptions, QueryFormat, QuerySource,
    SchemaContract, parse, parse_process, requested_color,
};

mod manual_coverage;

use super::{CLI_STYLES, MarkdownMathMode, help, uses_removed_section_option};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

mod actions;

mod markdown;

mod presentation;

mod queries;

mod sources;
