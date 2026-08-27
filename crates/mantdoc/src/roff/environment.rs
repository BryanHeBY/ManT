use std::collections::{BTreeMap, BTreeSet};

use crate::Limits;

/// One byte-preserving user macro body held by a single parse session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacroDefinition {
    pub(crate) lines: Vec<Vec<u8>>,
    pub(crate) appended: bool,
    pub(super) indirect: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NumberRegister {
    pub(super) value: i32,
    pub(super) increment: i32,
}

/// A semantic package scope that can temporarily override roff fill mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageFillScope {
    ManExample,
    MdocDisplay,
    MdocList,
}

/// Validation result for one `.tr` request before its pairs are installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TranslationRequestIssue {
    Empty,
    Odd { start: usize, end: usize },
}

/// Mutable data intentionally owned by exactly one roff parse session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Environment {
    pub(super) strings: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(super) implicit_empty_strings: BTreeSet<Vec<u8>>,
    pub(super) registers: BTreeMap<Vec<u8>, NumberRegister>,
    pub(super) macros: BTreeMap<Vec<u8>, MacroDefinition>,
    pub(super) translations: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(super) declared_characters: BTreeSet<Vec<u8>>,
    pub(super) character_definitions: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(super) suppressed_macro_names: BTreeSet<Vec<u8>>,
    pub(super) undefined_condition_names: BTreeSet<Vec<u8>>,
    pub(super) renamed_package_macros: BTreeMap<Vec<u8>, Vec<u8>>,
    pub(super) reported_nested_while_starts: BTreeSet<u32>,
    pub(super) definition_bytes: usize,
    pub(super) max_definitions: usize,
    pub(super) roff_no_fill: bool,
    pub(super) package_fill_scopes: Vec<(PackageFillScope, bool)>,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            strings: BTreeMap::new(),
            implicit_empty_strings: BTreeSet::new(),
            registers: BTreeMap::new(),
            macros: BTreeMap::new(),
            translations: BTreeMap::new(),
            declared_characters: BTreeSet::new(),
            character_definitions: BTreeMap::new(),
            suppressed_macro_names: BTreeSet::new(),
            undefined_condition_names: BTreeSet::new(),
            renamed_package_macros: BTreeMap::new(),
            reported_nested_while_starts: BTreeSet::new(),
            definition_bytes: 0,
            max_definitions: Limits::default().max_definitions,
            roff_no_fill: false,
            package_fill_scopes: Vec::new(),
        }
    }
}

/// A non-fatal failure while applying a roff environment operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentError {
    DefinitionLimit,
    DefinitionBytesLimit,
    RegisterExpression,
    ExpansionLimit,
    RecursionLimit,
    OutputLimit,
}

/// Result of expanding strings, registers, and macro arguments once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EnvironmentExpansion {
    pub(crate) bytes: Vec<u8>,
    pub(crate) steps: usize,
    pub(crate) missing_references: Vec<Vec<u8>>,
    pub(crate) malformed_escape_offsets: Vec<usize>,
}
