//! Per-parse roff execution environment and bounded delayed expansion.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Limits, numeric::evaluate_register_expression};

/// mandoc's small, read-only roff string compatibility catalog.
///
/// These values are looked up lazily rather than injected into each parse
/// environment, so they neither consume user definition budgets nor obscure
/// user-defined overrides.  Values intentionally remain roff byte spelling:
/// visible escape normalization owns their final presentation.
const PREDEFINED_STRINGS: &[(&[u8], &[u8])] = &[
    (b"Am", b"&"),
    (b"Ba", b"\\fR|\\fP"),
    (b"Ge", b"\\(>="),
    (b"Gt", b">"),
    (b"If", b"infinity"),
    (b"Le", b"\\(<="),
    (b"Lq", b"\\(lq"),
    (b"Lt", b"<"),
    (b"Na", b"NaN"),
    (b"Ne", b"\\(!="),
    (b"Pi", b"pi"),
    (b"Pm", b"\\(+-"),
    (b"Rq", b"\\(rq"),
    (b"left-bracket", b"["),
    (b"left-parenthesis", b"("),
    (b"lp", b"("),
    (b"left-singlequote", b"\\(oq"),
    (b"q", b"\\(dq"),
    (b"quote-left", b"\\(oq"),
    (b"quote-right", b"\\(cq"),
    (b"R", b"\\(rg"),
    (b"right-bracket", b"]"),
    (b"right-parenthesis", b")"),
    (b"rp", b")"),
    (b"right-singlequote", b"\\(cq"),
    (b"Tm", b"(Tm)"),
    (b"Px", b"POSIX"),
    (b"Ai", b"ANSI"),
    (b"'", b"\\'"),
    (b"aa", b"\\(aa"),
    (b"ga", b"\\(ga"),
    (b"`", b"\\`"),
    (b"lq", b"\\(lq"),
    (b"rq", b"\\(rq"),
    (b"ua", b"\\(ua"),
    (b"va", b"\\(va"),
    (b"<=", b"\\(<="),
    (b">=", b"\\(>="),
];

/// One byte-preserving user macro body held by a single parse session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacroDefinition {
    /// Physical body lines without their terminating newlines.
    pub(crate) lines: Vec<Vec<u8>>,
    /// Whether the current body was installed with `.am`/`.ami` rather than
    /// replacing the definition. Package macros retain their built-in action
    /// and execute this body as a suffix.
    pub(crate) appended: bool,
    /// Whether the name was resolved through `.dei`/`.ami` at definition
    /// time. Removing such a definition does not make a later spelling emit
    /// the ordinary deleted-user-macro recovery.
    indirect: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NumberRegister {
    // mandoc stores number registers in C `int`s.  Keep that fixed-width
    // arithmetic explicit: the legacy semantics wrap at the signed 32-bit
    // boundary rather than depending on either the host word size or Rust
    // debug/release overflow behavior.
    value: i32,
    increment: i32,
}

/// A semantic package scope that can temporarily override roff fill mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackageFillScope {
    /// A man(7) `.EX` display.
    ManExample,
    /// An mdoc(7) `.Bd` display.
    MdocDisplay,
    /// An mdoc(7) `.Bl` list.
    MdocList,
}

/// Validation result for one `.tr` request before its pairs are installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TranslationRequestIssue {
    /// The request did not contain any source glyph.
    Empty,
    /// The final glyph has no paired replacement and maps to a space.
    Odd { start: usize, end: usize },
}

/// Mutable data intentionally owned by exactly one roff parse session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Environment {
    strings: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Names whose first missing interpolation recovered as an empty string.
    ///
    /// This is deliberately distinct from an explicit `.ds name` entry.
    /// In particular, `.rn` only renames concrete definitions; mandoc keeps
    /// this recovery state attached to the spelling that was interpolated.
    implicit_empty_strings: BTreeSet<Vec<u8>>,
    registers: BTreeMap<Vec<u8>, NumberRegister>,
    macros: BTreeMap<Vec<u8>, MacroDefinition>,
    translations: BTreeMap<Vec<u8>, Vec<u8>>,
    declared_characters: BTreeSet<Vec<u8>>,
    character_definitions: BTreeMap<Vec<u8>, Vec<u8>>,
    suppressed_macro_names: BTreeSet<Vec<u8>>,
    /// Names observed as false `.if dname` predicates.  If such a name is
    /// later invoked as a control before a definition appears, mandoc treats
    /// it as an unknown user macro rather than a generic request element.
    undefined_condition_names: BTreeSet<Vec<u8>>,
    /// User-visible alias -> parser-dispatched package macro spelling.
    ///
    /// Package macros are not represented by a copy-mode body, so a set is
    /// insufficient: the parser must still dispatch the target's structural
    /// action when the renamed spelling is invoked.
    renamed_package_macros: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Nested `.while` source starts already reported while an outer loop is
    /// replayed.  One authored nested request may be visited once per outer
    /// iteration, but mandoc publishes its recovery only once.
    reported_nested_while_starts: BTreeSet<u32>,
    definition_bytes: usize,
    max_definitions: usize,
    roff_no_fill: bool,
    package_fill_scopes: Vec<(PackageFillScope, bool)>,
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
    /// A definition would exceed the shared definition-count budget.
    DefinitionLimit,
    /// A definition would exceed the shared definition-byte budget.
    DefinitionBytesLimit,
    /// A numeric register expression is not an integral basic-unit value.
    RegisterExpression,
    /// A recursive or repeatedly nested expansion reached the caller's budget.
    ExpansionLimit,
    /// Recursive string interpolation reached the bounded native call depth.
    RecursionLimit,
    /// Expanded bytes would exceed the caller's per-line output budget.
    OutputLimit,
}

/// Result of expanding strings, registers, and macro arguments once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EnvironmentExpansion {
    /// Expanded byte output; no lossy text decoding is performed here.
    pub(crate) bytes: Vec<u8>,
    /// Number of environment substitutions that consumed work budget.
    pub(crate) steps: usize,
    /// Missing names deliberately leave no synthetic bytes in the output.
    pub(crate) missing_references: Vec<Vec<u8>>,
    /// Source-relative starts of malformed `\\B`/`\\w` arguments that were
    /// still replaced with mandoc's deterministic recovery value.
    pub(crate) malformed_escape_offsets: Vec<usize>,
}

impl Environment {
    /// Return whether a nested-loop recovery at this physical source offset
    /// has not yet been published in this parse session.
    pub(crate) fn mark_nested_while_recovery(&mut self, start: u32) -> bool {
        self.reported_nested_while_starts.insert(start)
    }

    /// Record a `.char` name for later parser-stage validation.
    pub(crate) fn declare_character(&mut self, name: &[u8]) {
        self.declared_characters.insert(name.to_vec());
    }

    /// Whether a bracketed special name was declared by a prior `.char`.
    pub(crate) fn has_declared_character(&self, name: &[u8]) -> bool {
        self.declared_characters.contains(name)
    }

    /// Bind a valid `.char` name to its raw replacement bytes.
    pub(crate) fn define_character(&mut self, name: &[u8], value: Vec<u8>) {
        self.declare_character(name);
        self.character_definitions.insert(name.to_vec(), value);
    }

    /// Look up a valid `.char` replacement without applying host encoding.
    pub(crate) fn character_definition(&self, name: &[u8]) -> Option<&[u8]> {
        self.character_definitions.get(name).map(Vec::as_slice)
    }

    /// Record a macro name encountered only in an inactive conditional arm.
    pub(crate) fn suppress_macro_name(&mut self, name: &[u8]) {
        self.suppressed_macro_names.insert(name.to_vec());
    }

    /// Allow a later valid definition or rename to make a spelling callable.
    pub(crate) fn clear_suppressed_macro_name(&mut self, name: &[u8]) {
        self.suppressed_macro_names.remove(name);
    }

    /// Whether an unresolved name is known to have been skipped by a branch.
    pub(crate) fn is_suppressed_macro_name(&self, name: &[u8]) -> bool {
        self.suppressed_macro_names.contains(name)
    }

    /// Bind session-owned implicit register creation to the parser's limits.
    pub(crate) fn configure_limits(&mut self, limits: &Limits) {
        self.max_definitions = limits.max_definitions;
    }

    /// Record whether subsequent visible package text is in no-fill mode.
    pub(crate) fn no_fill(&mut self, value: bool) {
        self.roff_no_fill = value;
    }

    /// Enter a semantic package scope, retaining whether it suppresses fill.
    pub(crate) fn push_package_fill_scope(&mut self, scope: PackageFillScope, no_fill: bool) {
        self.package_fill_scopes.push((scope, no_fill));
    }

    /// Exit the most-recent semantic package scope of a matching kind.
    ///
    /// Broken manuals may use a closer out of nesting order; ignoring an
    /// unmatched closer keeps recovery local and preserves independent scopes.
    pub(crate) fn pop_package_fill_scope(&mut self, scope: PackageFillScope) {
        if let Some(index) = self
            .package_fill_scopes
            .iter()
            .rposition(|(candidate, _)| *candidate == scope)
        {
            self.package_fill_scopes.remove(index);
        }
    }

    /// Whether visible package text is currently laid out in fill mode.
    pub(crate) fn is_filled(&self) -> bool {
        !self.roff_no_fill && self.package_fill_scopes.iter().all(|(_, no_fill)| !no_fill)
    }

    /// Add or replace a delayed string definition.
    pub(crate) fn define_string(
        &mut self,
        name: &[u8],
        value: &[u8],
        append: bool,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        let existing = self.strings.get(name).map_or(0, Vec::len);
        let definitions = self.definition_count()
            + usize::from(
                !self.strings.contains_key(name) && !self.implicit_empty_strings.contains(name),
            );
        if definitions > limits.max_definitions {
            return Err(EnvironmentError::DefinitionLimit);
        }
        let retained = if append {
            self.definition_bytes.checked_add(value.len())
        } else {
            self.definition_bytes
                .checked_sub(existing)
                .and_then(|total| total.checked_add(value.len()))
        }
        .ok_or(EnvironmentError::DefinitionBytesLimit)?;
        if retained > limits.max_definition_bytes {
            return Err(EnvironmentError::DefinitionBytesLimit);
        }
        let entry = self.strings.entry(name.to_vec()).or_default();
        if append {
            entry.extend_from_slice(value);
        } else {
            entry.clear();
            entry.extend_from_slice(value);
        }
        self.definition_bytes = retained;
        self.implicit_empty_strings.remove(name);
        Ok(())
    }

    /// Define one integer basic-unit register, honoring leading relative signs.
    pub(crate) fn define_register(
        &mut self,
        name: &[u8],
        expression: &[u8],
        increment: Option<&[u8]>,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        let Some(parsed) = parse_basic_integer(expression, true, true)? else {
            return Ok(());
        };
        let parsed_value = wrapping_i64_to_i32(parsed.value);
        let value = match parsed.relative {
            Some(1) => self
                .registers
                .get(name)
                .map_or(0, |register| register.value)
                .wrapping_add(parsed_value),
            Some(-1) => self
                .registers
                .get(name)
                .map_or(0, |register| register.value)
                .wrapping_sub(parsed_value),
            Some(_) => return Err(EnvironmentError::RegisterExpression),
            None => parsed_value,
        };
        let increment = match increment {
            Some(increment) => parse_basic_integer(increment, false, false)?.map_or_else(
                || {
                    self.registers
                        .get(name)
                        .map_or(0, |register| register.increment)
                },
                |increment| wrapping_i64_to_i32(increment.value),
            ),
            None => self
                .registers
                .get(name)
                .map_or(0, |register| register.increment),
        };
        self.replace_register(name, NumberRegister { value, increment }, limits)
    }

    /// Add or replace a macro body without expanding its copy-mode bytes.
    pub(crate) fn define_macro(
        &mut self,
        name: &[u8],
        lines: Vec<Vec<u8>>,
        append: bool,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        self.define_macro_with_origin(name, lines, append, false, limits)
    }

    /// Define a macro whose name was resolved through `.dei` or `.ami`.
    pub(crate) fn define_indirect_macro(
        &mut self,
        name: &[u8],
        lines: Vec<Vec<u8>>,
        append: bool,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        self.define_macro_with_origin(name, lines, append, true, limits)
    }

    fn define_macro_with_origin(
        &mut self,
        name: &[u8],
        lines: Vec<Vec<u8>>,
        append: bool,
        indirect: bool,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        self.clear_suppressed_macro_name(name);
        let bytes = lines
            .iter()
            .try_fold(0_usize, |total, line| total.checked_add(line.len()))
            .ok_or(EnvironmentError::DefinitionBytesLimit)?;
        let existing = self
            .macros
            .get(name)
            .map_or(0, |definition| definition.lines.iter().map(Vec::len).sum());
        let definitions = self.definition_count()
            + usize::from(
                !self.macros.contains_key(name) && !self.implicit_empty_strings.contains(name),
            );
        if definitions > limits.max_definitions {
            return Err(EnvironmentError::DefinitionLimit);
        }
        let additional = if append {
            bytes
        } else {
            bytes.saturating_sub(existing)
        };
        let retained = if append {
            self.definition_bytes.checked_add(additional)
        } else {
            self.definition_bytes
                .checked_sub(existing)
                .and_then(|total| total.checked_add(bytes))
        }
        .ok_or(EnvironmentError::DefinitionBytesLimit)?;
        if retained > limits.max_definition_bytes {
            return Err(EnvironmentError::DefinitionBytesLimit);
        }
        let definition = self.macros.entry(name.to_vec()).or_insert(MacroDefinition {
            lines: Vec::new(),
            appended: false,
            indirect,
        });
        if append {
            definition.lines.extend(lines);
            definition.appended = true;
        } else {
            definition.lines = lines;
            definition.appended = false;
            definition.indirect = indirect;
        }
        self.definition_bytes = retained;
        self.implicit_empty_strings.remove(name);
        Ok(())
    }

    /// Rename a macro, string, or register without merging unrelated state.
    pub(crate) fn rename(&mut self, old: &[u8], new: &[u8]) {
        if old == new {
            return;
        }
        // Roff renaming replaces a same-kind destination. Reclaim its retained
        // bytes first so the shared definition budget remains an invariant.
        self.remove(new);
        if let Some(value) = self.strings.remove(old) {
            self.strings.insert(new.to_vec(), value);
        }
        if let Some(value) = self.registers.remove(old) {
            self.registers.insert(new.to_vec(), value);
        }
        if let Some(value) = self.macros.remove(old) {
            self.macros.insert(new.to_vec(), value);
        }
    }

    /// Rename a parser-dispatched package macro without manufacturing a
    /// user-macro body.  The alias remains observable to `d` conditions and
    /// retains the target's structural parser action at invocation time.
    pub(crate) fn rename_package_macro(&mut self, old: &[u8], new: &[u8]) {
        self.remove(new);
        self.renamed_package_macros
            .insert(new.to_vec(), old.to_vec());
    }

    /// Return the original package macro dispatched by a renamed spelling.
    pub(crate) fn renamed_package_macro(&self, name: &[u8]) -> Option<&[u8]> {
        self.renamed_package_macros.get(name).map(Vec::as_slice)
    }

    /// Create an independent macro alias by copying its byte-preserving body.
    pub(crate) fn alias_macro(
        &mut self,
        target: &[u8],
        alias: &[u8],
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        let Some(definition) = self.macros.get(target).cloned() else {
            return Ok(());
        };
        self.define_macro_with_origin(alias, definition.lines, false, definition.indirect, limits)
    }

    /// Remove one definition of any environment kind.
    pub(crate) fn remove(&mut self, name: &[u8]) {
        if let Some(value) = self.strings.remove(name) {
            self.definition_bytes = self.definition_bytes.saturating_sub(value.len());
        }
        self.registers.remove(name);
        if let Some(definition) = self.macros.remove(name) {
            let bytes = definition.lines.iter().map(Vec::len).sum::<usize>();
            self.definition_bytes = self.definition_bytes.saturating_sub(bytes);
        }
        self.implicit_empty_strings.remove(name);
        self.renamed_package_macros.remove(name);
    }

    /// Whether `.rm` should diagnose a later invocation of this direct user
    /// macro after it removes the definition.
    pub(crate) fn macro_removal_is_diagnosable(&self, name: &[u8]) -> bool {
        self.macros
            .get(name)
            .is_some_and(|definition| !definition.indirect)
    }

    /// Remove only a number register, leaving same-named strings and macros intact.
    pub(crate) fn remove_register(&mut self, name: &[u8]) {
        self.registers.remove(name);
    }

    /// Return whether a number register has an explicit definition in this session.
    pub(crate) fn is_register_defined(&self, name: &[u8]) -> bool {
        predefined_register(name).is_some() || self.registers.contains_key(name)
    }

    /// Read a register without materializing an undefined entry or advancing
    /// its increment.  Package parsers use this for private execution state
    /// such as mdoc's `nS` synopsis register.
    pub(crate) fn register_value(&self, name: &[u8]) -> Option<i32> {
        predefined_register(name)
            .or_else(|| self.registers.get(name).map(|register| register.value))
    }

    /// Return whether a string or user macro has an explicit definition in this session.
    pub(crate) fn is_name_defined(&self, name: &[u8]) -> bool {
        self.strings.contains_key(name)
            || self.implicit_empty_strings.contains(name)
            || self.macros.contains_key(name)
            || self.renamed_package_macros.contains_key(name)
            // mandoc recognizes the default device-name string as defined,
            // but preserves its interpolation spelling in the public AST.
            || name == b".T"
            || predefined_string(name).is_some()
    }

    /// Remember a false name-defined condition for later control recovery.
    pub(crate) fn observe_undefined_name_condition(&mut self, name: &[u8]) {
        self.undefined_condition_names.insert(name.to_vec());
    }

    /// Whether a prior false `dname` condition makes this undefined control a
    /// recoverable unknown user macro.
    pub(crate) fn is_conditionally_unknown_macro(&self, name: &[u8]) -> bool {
        self.undefined_condition_names.contains(name) && !self.is_name_defined(name)
    }

    /// Resolve one string name for roff's indirect definition requests.
    ///
    /// Unlike ordinary source expansion, `.dei`/`.ami` take names of strings
    /// rather than an interpolation spelling at this request boundary.
    pub(crate) fn indirect_string(&self, name: &[u8]) -> Option<Vec<u8>> {
        self.string_value(name).map(ToOwned::to_owned)
    }

    /// Borrow a macro body so the executor can push a bounded argument frame.
    pub(crate) fn macro_definition(&self, name: &[u8]) -> Option<&MacroDefinition> {
        self.macros.get(name)
    }

    /// Whether a user string exists with an empty value. Roff permits such a
    /// name in control position and mandoc treats that invocation as a silent
    /// zero-length macro.
    pub(crate) fn is_empty_string(&self, name: &[u8]) -> bool {
        self.implicit_empty_strings.contains(name)
            || self.strings.get(name).is_some_and(Vec::is_empty)
    }

    /// Record the persistent empty-value recovery used after an undefined
    /// string interpolation.  It has no stored bytes and does not participate
    /// in `.rn`, but it does have to remain bounded like every other name that
    /// can be introduced by untrusted input.
    pub(crate) fn materialize_implicit_empty_string(
        &mut self,
        name: &[u8],
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        if self.strings.contains_key(name) || self.implicit_empty_strings.contains(name) {
            return Ok(());
        }
        if self.definition_count() >= limits.max_definitions {
            return Err(EnvironmentError::DefinitionLimit);
        }
        self.implicit_empty_strings.insert(name.to_vec());
        Ok(())
    }

    /// Whether a user macro body supplements, rather than replaces, a package
    /// macro invocation.
    pub(crate) fn has_appended_macro_definition(&self, name: &[u8]) -> bool {
        self.macros
            .get(name)
            .is_some_and(|definition| definition.appended)
    }

    /// Apply a single-byte `.tr` table to ordinary text.  Roff escapes remain
    /// opaque here: their semantic decoding belongs to the escape normalizer,
    /// and translating their spelling would corrupt the escape grammar.
    pub(crate) fn translate_text(
        &self,
        bytes: &[u8],
        escape: u8,
        maximum_output_bytes: usize,
    ) -> Result<Vec<u8>, EnvironmentError> {
        if self.translations.is_empty() {
            return (bytes.len() <= maximum_output_bytes)
                .then(|| bytes.to_vec())
                .ok_or(EnvironmentError::OutputLimit);
        }
        let mut translated = Vec::with_capacity(bytes.len());
        let mut cursor = 0;
        while cursor < bytes.len() {
            let end = if bytes[cursor] == escape {
                escape_end(bytes, cursor, escape).unwrap_or(bytes.len())
            } else {
                cursor + 1
            };
            let glyph = &bytes[cursor..end];
            let output = self.translations.get(glyph).map_or(glyph, Vec::as_slice);
            let next = translated
                .len()
                .checked_add(output.len())
                .ok_or(EnvironmentError::OutputLimit)?;
            if next > maximum_output_bytes {
                return Err(EnvironmentError::OutputLimit);
            }
            translated.extend_from_slice(output);
            cursor = end;
        }
        Ok(translated)
    }

    /// Replace output glyphs in ordered pairs.  A trailing unmatched glyph is
    /// translated to a space, matching mandoc's `.tr` rule.
    pub(crate) fn define_translation(&mut self, glyphs: &[u8], escape: u8) {
        let mut cursor = 0;
        while cursor < glyphs.len() {
            let first_end = glyph_end(glyphs, cursor, escape);
            let first = glyphs[cursor..first_end].to_vec();
            cursor = first_end;
            let second = if cursor == glyphs.len() {
                vec![b' ']
            } else {
                let second_end = glyph_end(glyphs, cursor, escape);
                let second = glyphs[cursor..second_end].to_vec();
                cursor = second_end;
                second
            };
            self.translations.insert(first, second);
        }
    }

    /// Expand scanner-visible environment escapes once, retaining unexpanded
    /// ordinary roff escapes for the dedicated visible-text normalizer.
    pub(crate) fn expand(
        &mut self,
        bytes: &[u8],
        escape: u8,
        arguments: &[Vec<u8>],
        remaining_steps: usize,
        maximum_output_bytes: usize,
    ) -> Result<EnvironmentExpansion, EnvironmentError> {
        self.expand_with_copy_mode(
            bytes,
            escape,
            arguments,
            remaining_steps,
            maximum_output_bytes,
            false,
            0,
        )
    }

    /// Expand a copy-mode definition.  String interpolations freeze at the
    /// definition site, while macro arguments and number registers remain
    /// delayed until a later macro invocation.
    pub(crate) fn expand_copy_mode_definition(
        &mut self,
        bytes: &[u8],
        escape: u8,
        remaining_steps: usize,
        maximum_output_bytes: usize,
    ) -> Result<EnvironmentExpansion, EnvironmentError> {
        self.expand_with_copy_mode(
            bytes,
            escape,
            &[],
            remaining_steps,
            maximum_output_bytes,
            true,
            0,
        )
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Escape families share one ordered, bounded pass.
    fn expand_with_copy_mode(
        &mut self,
        bytes: &[u8],
        escape: u8,
        arguments: &[Vec<u8>],
        remaining_steps: usize,
        maximum_output_bytes: usize,
        copy_mode_definition: bool,
        recursion_depth: usize,
    ) -> Result<EnvironmentExpansion, EnvironmentError> {
        // Environment definitions are attacker-controlled and may be cyclic.
        // Keep recursive interpolation beneath the same finite recovery class
        // as its shared work budget instead of relying on the host stack.
        if recursion_depth >= 256 {
            return Err(EnvironmentError::RecursionLimit);
        }
        let mut output = Vec::with_capacity(bytes.len());
        let mut missing_references = Vec::new();
        let mut malformed_escape_offsets = Vec::new();
        let mut cursor = 0;
        let mut steps = 0_usize;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if byte != escape || cursor + 1 == bytes.len() {
                push_expanded_bytes(&mut output, &[byte], maximum_output_bytes)?;
                cursor += 1;
                continue;
            }
            let escape_start = cursor;
            let kind = bytes[cursor + 1];
            match kind {
                b'B' | b'w' => {
                    let argument_start = cursor + 2;
                    let parsed = read_delimited_escape_argument(bytes, argument_start, escape);
                    let (argument, end, malformed) = parsed.map_or_else(
                        || {
                            let argument = bytes
                                .get(argument_start + 1..)
                                .filter(|_| bytes.get(argument_start).is_some())
                                .unwrap_or_default();
                            (argument, bytes.len(), true)
                        },
                        |(argument, end)| (argument, end, false),
                    );
                    steps = steps
                        .checked_add(1)
                        .ok_or(EnvironmentError::ExpansionLimit)?;
                    if steps > remaining_steps {
                        return Err(EnvironmentError::ExpansionLimit);
                    }
                    if malformed {
                        malformed_escape_offsets.push(escape_start);
                    }
                    if kind == b'B' {
                        let value = if !malformed && validates_roff_number(argument) {
                            b"1".as_slice()
                        } else {
                            b"0".as_slice()
                        };
                        push_expanded_bytes(&mut output, value, maximum_output_bytes)?;
                    } else {
                        let width = glyph_count(argument, escape)
                            .checked_mul(24)
                            .ok_or(EnvironmentError::OutputLimit)?;
                        push_expanded_bytes(
                            &mut output,
                            width.to_string().as_bytes(),
                            maximum_output_bytes,
                        )?;
                    }
                    cursor = end;
                }
                b'n' | b'$' if copy_mode_definition => {
                    push_expanded_bytes(
                        &mut output,
                        &bytes[escape_start..cursor + 2],
                        maximum_output_bytes,
                    )?;
                    cursor += 2;
                }
                b'*' | b'n' => {
                    cursor += 2;
                    let adjustment = if kind == b'n' {
                        match bytes.get(cursor).copied() {
                            Some(b'+') => {
                                cursor += 1;
                                Some(1_i64)
                            }
                            Some(b'-') => {
                                cursor += 1;
                                Some(-1_i64)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let Some((name, consumed)) = read_name(&bytes[cursor..]) else {
                        if kind == b'*' && bytes.get(cursor) == Some(&b'[') {
                            // An unterminated bracketed string name consumes
                            // the physical-line tail.  The parser emits the
                            // companion invalid-escape finding from the raw
                            // spelling, while this layer preserves the
                            // ordinary empty-string recovery and its state.
                            let name = bytes[cursor.saturating_add(1)..].to_vec();
                            if !name.is_empty() {
                                missing_references.push(name);
                            }
                            break;
                        }
                        if kind == b'n' && bytes.get(cursor) == Some(&b'[') {
                            // An unterminated bracketed number-register
                            // reference consumes the remainder of the input
                            // line.  Its preceding separator is validator
                            // trailing whitespace, not published text.
                            while matches!(output.last(), Some(b' ' | b'\t')) {
                                output.pop();
                            }
                            break;
                        }
                        push_expanded_bytes(
                            &mut output,
                            &bytes[escape_start..cursor],
                            maximum_output_bytes,
                        )?;
                        continue;
                    };
                    cursor += consumed;
                    steps = steps
                        .checked_add(1)
                        .ok_or(EnvironmentError::ExpansionLimit)?;
                    if steps > remaining_steps {
                        return Err(EnvironmentError::ExpansionLimit);
                    }
                    let resolved_name = if contains_reference_escape(name, escape)
                        || (kind == b'*' && contains_literal_string_name_escape(name, escape))
                    {
                        let resolved = self.resolve_reference_name(
                            name,
                            escape,
                            arguments,
                            remaining_steps.saturating_sub(steps),
                            maximum_output_bytes,
                        )?;
                        steps = steps
                            .checked_add(resolved.steps)
                            .ok_or(EnvironmentError::ExpansionLimit)?;
                        if steps > remaining_steps {
                            return Err(EnvironmentError::ExpansionLimit);
                        }
                        missing_references.extend(resolved.missing_references);
                        resolved.bytes
                    } else {
                        name.to_vec()
                    };
                    if kind == b'*' {
                        if let Some(value) =
                            self.string_value(&resolved_name).map(ToOwned::to_owned)
                        {
                            let nested = self.expand_with_copy_mode(
                                &value,
                                escape,
                                arguments,
                                remaining_steps.saturating_sub(steps),
                                maximum_output_bytes,
                                copy_mode_definition,
                                recursion_depth + 1,
                            )?;
                            steps = steps
                                .checked_add(nested.steps)
                                .ok_or(EnvironmentError::ExpansionLimit)?;
                            if steps > remaining_steps {
                                return Err(EnvironmentError::ExpansionLimit);
                            }
                            missing_references.extend(nested.missing_references);
                            malformed_escape_offsets.extend(nested.malformed_escape_offsets);
                            push_expanded_bytes(&mut output, &nested.bytes, maximum_output_bytes)?;
                        } else if resolved_name == b".T" {
                            // The default formatter device is semantically
                            // available as `ascii`, yet libmandoc-rs keeps
                            // both string spellings in its owned AST. Keep
                            // the authored escape and suppress an undefined
                            // reference warning; a user `.ds .T value` above
                            // still takes precedence and expands normally.
                            push_expanded_bytes(
                                &mut output,
                                &bytes[escape_start..cursor],
                                maximum_output_bytes,
                            )?;
                        } else if self.macros.contains_key(&resolved_name) {
                            // String and macro names share the roff
                            // definition namespace. Interpolating a macro as
                            // a string is a defined zero-length value, not an
                            // undefined reference.
                        } else {
                            missing_references.push(resolved_name);
                        }
                    } else if matches!(resolved_name.as_slice(), b"$" | b".$") {
                        push_expanded_bytes(
                            &mut output,
                            arguments.len().to_string().as_bytes(),
                            maximum_output_bytes,
                        )?;
                    } else if let Some(value) = predefined_register(&resolved_name) {
                        push_expanded_bytes(
                            &mut output,
                            value.to_string().as_bytes(),
                            maximum_output_bytes,
                        )?;
                    } else if let Some(value) = self.registers.get_mut(&resolved_name) {
                        if let Some(adjustment) = adjustment {
                            value.value = match adjustment {
                                1 => value.value.wrapping_add(value.increment),
                                -1 => value.value.wrapping_sub(value.increment),
                                _ => return Err(EnvironmentError::RegisterExpression),
                            };
                        }
                        push_expanded_bytes(
                            &mut output,
                            value.value.to_string().as_bytes(),
                            maximum_output_bytes,
                        )?;
                    } else {
                        // mandoc materializes an undefined number register as
                        // zero on first interpolation.  That state changes a
                        // later `rname` conditional and can subsequently be
                        // removed with `.rr`.
                        self.materialize_register(&resolved_name)?;
                        push_expanded_bytes(&mut output, b"0", maximum_output_bytes)?;
                    }
                }
                b'$' => {
                    cursor += 2;
                    let Some(index) = bytes.get(cursor).copied() else {
                        push_expanded_bytes(
                            &mut output,
                            &bytes[escape_start..cursor],
                            maximum_output_bytes,
                        )?;
                        continue;
                    };
                    cursor += 1;
                    if index.is_ascii_digit() && index != b'0' {
                        steps = steps
                            .checked_add(1)
                            .ok_or(EnvironmentError::ExpansionLimit)?;
                        if steps > remaining_steps {
                            return Err(EnvironmentError::ExpansionLimit);
                        }
                        if let Some(value) = arguments.get(usize::from(index - b'1')) {
                            let nested = self.expand_with_copy_mode(
                                value,
                                escape,
                                arguments,
                                remaining_steps.saturating_sub(steps),
                                maximum_output_bytes,
                                false,
                                recursion_depth + 1,
                            )?;
                            steps = steps
                                .checked_add(nested.steps)
                                .ok_or(EnvironmentError::ExpansionLimit)?;
                            if steps > remaining_steps {
                                return Err(EnvironmentError::ExpansionLimit);
                            }
                            missing_references.extend(nested.missing_references);
                            malformed_escape_offsets.extend(nested.malformed_escape_offsets);
                            push_expanded_bytes(&mut output, &nested.bytes, maximum_output_bytes)?;
                        }
                    } else if matches!(index, b'*' | b'@') {
                        steps = steps
                            .checked_add(1)
                            .ok_or(EnvironmentError::ExpansionLimit)?;
                        if steps > remaining_steps {
                            return Err(EnvironmentError::ExpansionLimit);
                        }
                        for (position, argument) in arguments.iter().enumerate() {
                            if position > 0 {
                                push_expanded_bytes(&mut output, b" ", maximum_output_bytes)?;
                            }
                            let nested = self.expand_with_copy_mode(
                                argument,
                                escape,
                                arguments,
                                remaining_steps.saturating_sub(steps),
                                maximum_output_bytes,
                                false,
                                recursion_depth + 1,
                            )?;
                            steps = steps
                                .checked_add(nested.steps)
                                .ok_or(EnvironmentError::ExpansionLimit)?;
                            if steps > remaining_steps {
                                return Err(EnvironmentError::ExpansionLimit);
                            }
                            missing_references.extend(nested.missing_references);
                            malformed_escape_offsets.extend(nested.malformed_escape_offsets);
                            push_expanded_bytes(&mut output, &nested.bytes, maximum_output_bytes)?;
                        }
                    } else {
                        push_expanded_bytes(
                            &mut output,
                            &[escape, kind, index],
                            maximum_output_bytes,
                        )?;
                    }
                }
                _ => {
                    push_expanded_bytes(
                        &mut output,
                        &bytes[cursor..cursor + 2],
                        maximum_output_bytes,
                    )?;
                    cursor += 2;
                }
            }
        }
        Ok(EnvironmentExpansion {
            bytes: output,
            steps,
            missing_references,
            malformed_escape_offsets,
        })
    }

    fn replace_register(
        &mut self,
        name: &[u8],
        value: NumberRegister,
        limits: &Limits,
    ) -> Result<(), EnvironmentError> {
        let definitions = self.definition_count() + usize::from(!self.registers.contains_key(name));
        if definitions > limits.max_definitions {
            return Err(EnvironmentError::DefinitionLimit);
        }
        self.registers.insert(name.to_vec(), value);
        Ok(())
    }

    fn materialize_register(&mut self, name: &[u8]) -> Result<(), EnvironmentError> {
        if self.registers.contains_key(name) {
            return Ok(());
        }
        if self.definition_count() >= self.max_definitions {
            return Err(EnvironmentError::DefinitionLimit);
        }
        self.registers.insert(
            name.to_vec(),
            NumberRegister {
                value: 0,
                increment: 0,
            },
        );
        Ok(())
    }

    fn definition_count(&self) -> usize {
        self.strings.len()
            + self.implicit_empty_strings.len()
            + self.registers.len()
            + self.macros.len()
    }

    /// Look up a user value first, then a zero-cost read-only compatibility
    /// string.  User values deliberately override predefined ones.
    fn string_value(&self, name: &[u8]) -> Option<&[u8]> {
        self.strings
            .get(name)
            .map(Vec::as_slice)
            .or_else(|| {
                self.implicit_empty_strings
                    .contains(name)
                    .then_some(b"".as_slice())
            })
            .or_else(|| predefined_string(name))
    }

    /// Resolve delayed string/register escapes appearing *inside* a bracketed
    /// string or register name.  The loop walks right-to-left, matching
    /// mandoc's expansion order while remaining stack-safe for attacker-owned
    /// nesting.  It intentionally handles only parser-visible references and
    /// literal escape spellings; ordinary visible escape normalization still
    /// belongs to the later escape stage.
    fn resolve_reference_name(
        &mut self,
        name: &[u8],
        escape: u8,
        arguments: &[Vec<u8>],
        remaining_steps: usize,
        maximum_output_bytes: usize,
    ) -> Result<NameExpansion, EnvironmentError> {
        let mut bytes = name.to_vec();
        let mut cursor = bytes.len();
        let mut steps = 0_usize;
        let mut missing_references = Vec::new();
        while cursor > 0 {
            cursor -= 1;
            if bytes[cursor] != escape || escaped_by_previous(&bytes, cursor, escape) {
                continue;
            }
            let Some(kind) = bytes.get(cursor + 1).copied() else {
                continue;
            };
            if matches!(kind, b'\\' | b'e') {
                bytes.splice(cursor..cursor + 2, [escape]);
                continue;
            }
            if !matches!(kind, b'*' | b'n') {
                continue;
            }
            let mut name_cursor = cursor + 2;
            let adjustment = if kind == b'n' {
                match bytes.get(name_cursor).copied() {
                    Some(b'+') => {
                        name_cursor += 1;
                        Some(1_i8)
                    }
                    Some(b'-') => {
                        name_cursor += 1;
                        Some(-1_i8)
                    }
                    _ => None,
                }
            } else {
                None
            };
            let Some((reference, consumed)) = read_name(&bytes[name_cursor..]) else {
                continue;
            };
            let end = name_cursor + consumed;
            steps = steps
                .checked_add(1)
                .ok_or(EnvironmentError::ExpansionLimit)?;
            if steps > remaining_steps {
                return Err(EnvironmentError::ExpansionLimit);
            }
            let replacement = if kind == b'*' {
                if let Some(value) = self.string_value(reference) {
                    value.to_vec()
                } else {
                    missing_references.push(reference.to_vec());
                    Vec::new()
                }
            } else if matches!(reference, b"$" | b".$") {
                arguments.len().to_string().into_bytes()
            } else if let Some(value) = predefined_register(reference) {
                value.to_string().into_bytes()
            } else if let Some(value) = self.registers.get_mut(reference) {
                if let Some(adjustment) = adjustment {
                    value.value = match adjustment {
                        1 => value.value.wrapping_add(value.increment),
                        -1 => value.value.wrapping_sub(value.increment),
                        _ => return Err(EnvironmentError::RegisterExpression),
                    };
                }
                value.value.to_string().into_bytes()
            } else {
                self.materialize_register(reference)?;
                b"0".to_vec()
            };
            let length = bytes
                .len()
                .checked_sub(end - cursor)
                .and_then(|length| length.checked_add(replacement.len()))
                .ok_or(EnvironmentError::OutputLimit)?;
            if length > maximum_output_bytes {
                return Err(EnvironmentError::OutputLimit);
            }
            bytes.splice(cursor..end, replacement);
        }
        Ok(NameExpansion {
            bytes,
            steps,
            missing_references,
        })
    }
}

/// Return the validator-visible exceptional form of a translation request.
///
/// The executor still installs an odd final glyph with a space replacement;
/// callers use this result solely to retain mandoc's source diagnostic.
pub(crate) fn translation_request_issue(
    glyphs: &[u8],
    escape: u8,
) -> Option<TranslationRequestIssue> {
    if glyphs.is_empty() {
        return Some(TranslationRequestIssue::Empty);
    }
    let mut cursor = 0;
    while cursor < glyphs.len() {
        let first_start = cursor;
        let first_end = glyph_end(glyphs, cursor, escape);
        cursor = first_end;
        if cursor == glyphs.len() {
            return Some(TranslationRequestIssue::Odd {
                start: first_start,
                end: first_end,
            });
        }
        cursor = glyph_end(glyphs, cursor, escape);
    }
    None
}

fn predefined_string(name: &[u8]) -> Option<&'static [u8]> {
    PREDEFINED_STRINGS
        .iter()
        .find_map(|(candidate, value)| (*candidate == name).then_some(*value))
}

/// mandoc's deterministic, read-only two-character number registers.
///
/// They are resolved before user definitions, matching `roff_getregro()` in
/// the legacy engine.  Consequently `.nr .A 111` may retain a user entry for
/// compatibility bookkeeping but interpolation still observes the fixed
/// value `0`.
fn predefined_register(name: &[u8]) -> Option<i32> {
    match name {
        b".A" | b".j" => Some(0),
        b".g" | b".T" => Some(1),
        b".H" => Some(24),
        b".V" => Some(40),
        _ => None,
    }
}

/// Convert legacy unbounded integer evaluation into roff's `int` register
/// storage without a target-dependent narrowing cast.
fn wrapping_i64_to_i32(value: i64) -> i32 {
    let bits = u32::try_from(value.rem_euclid(1_i64 << 32))
        .expect("modulo 2^32 always fits the public register representation");
    i32::from_ne_bytes(bits.to_ne_bytes())
}

struct NameExpansion {
    bytes: Vec<u8>,
    steps: usize,
    missing_references: Vec<Vec<u8>>,
}

fn glyph_end(bytes: &[u8], start: usize, escape: u8) -> usize {
    if bytes.get(start) == Some(&escape) {
        escape_end(bytes, start, escape).unwrap_or(bytes.len())
    } else {
        start.saturating_add(1).min(bytes.len())
    }
}

/// Read one arbitrary-delimiter escape argument, skipping nested escape
/// spellings so a quoted delimiter inside a child escape does not terminate
/// the outer argument.  The returned end includes the closing delimiter.
fn read_delimited_escape_argument(
    bytes: &[u8],
    delimiter_position: usize,
    escape: u8,
) -> Option<(&[u8], usize)> {
    let delimiter = *bytes.get(delimiter_position)?;
    let start = delimiter_position + 1;
    let mut cursor = start;
    while cursor < bytes.len() {
        if bytes[cursor] == delimiter {
            return Some((&bytes[start..cursor], cursor + 1));
        }
        if bytes[cursor] == escape {
            cursor = escape_end(bytes, cursor, escape)?;
        } else {
            cursor += 1;
        }
    }
    None
}

/// Count the terminal-visible glyphs in mandoc's device-independent `\\w`
/// approximation.  A formatter-only escape has no width; any other complete
/// escape represents one glyph regardless of its source byte length.
fn glyph_count(bytes: &[u8], escape: u8) -> usize {
    let mut cursor = 0;
    let mut count = 0_usize;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            count += 1;
            cursor += 1;
            continue;
        }
        let Some(kind) = bytes.get(cursor + 1).copied() else {
            count += 1;
            break;
        };
        cursor = escape_end(bytes, cursor, escape).unwrap_or(bytes.len());
        if !matches!(
            kind,
            b'%' | b'&'
                | b')'
                | b','
                | b'/'
                | b'^'
                | b'a'
                | b'd'
                | b'r'
                | b't'
                | b'u'
                | b'{'
                | b'|'
                | b'}'
                | b':'
                | b'f'
                | b's'
        ) {
            count += 1;
        }
    }
    count
}

/// Validate the bounded numeric subset used by mandoc's `\\B` expansion.
///
/// The existing `.nr` evaluator provides deterministic arithmetic and
/// parentheses.  This wrapper additionally rejects unknown bytes and
/// unbalanced parentheses, because `\\B` answers whether its entire argument
/// is a number rather than accepting a valid prefix.
fn validates_roff_number(bytes: &[u8]) -> bool {
    if bytes.is_empty()
        || bytes.iter().any(|byte| {
            !matches!(
                *byte,
                b'0'..=b'9'
                    | b' '
                    | b'\t'
                    | b'+'
                    | b'-'
                    | b'*'
                    | b'/'
                    | b'%'
                    | b'&'
                    | b':'
                    | b'<'
                    | b'>'
                    | b'='
                    | b'!'
                    | b'('
                    | b')'
                    | b'.'
                    | b'f'
                    | b'i'
                    | b'c'
                    | b'v'
                    | b'P'
                    | b'm'
                    | b'n'
                    | b'p'
                    | b'u'
                    | b'M'
            )
        })
    {
        return false;
    }

    let mut depth = 0_usize;
    for byte in bytes {
        match byte {
            b'(' => depth = depth.saturating_add(1),
            b')' => match depth.checked_sub(1) {
                Some(next) => depth = next,
                None => return false,
            },
            _ => {}
        }
    }
    depth == 0 && matches!(evaluate_register_expression(bytes, true, true), Ok(Some(_)))
}

fn escape_end(bytes: &[u8], start: usize, escape: u8) -> Option<usize> {
    (bytes.get(start) == Some(&escape)).then_some(())?;
    let kind = *bytes.get(start + 1)?;
    match kind {
        b'(' => Some((start + 4).min(bytes.len())),
        b'[' => bytes[start + 2..]
            .iter()
            .position(|byte| *byte == b']')
            .map(|offset| start + 3 + offset),
        b'*' | b'n' => match bytes.get(start + 2) {
            Some(b'[') => bytes[start + 3..]
                .iter()
                .position(|byte| *byte == b']')
                .map(|offset| start + 4 + offset),
            Some(b'(') => Some((start + 5).min(bytes.len())),
            Some(_) => Some((start + 3).min(bytes.len())),
            None => Some(bytes.len()),
        },
        _ => Some((start + 2).min(bytes.len())),
    }
}

struct BasicInteger {
    value: i64,
    relative: Option<i8>,
}

fn parse_basic_integer(
    expression: &[u8],
    scale: bool,
    leading_relative: bool,
) -> Result<Option<BasicInteger>, EnvironmentError> {
    let parsed = evaluate_register_expression(expression, scale, leading_relative)
        .map_err(|_| EnvironmentError::RegisterExpression)?;
    Ok(parsed.map(|parsed| BasicInteger {
        value: parsed.value,
        relative: parsed.relative,
    }))
}

fn push_expanded_bytes(
    output: &mut Vec<u8>,
    bytes: &[u8],
    maximum_output_bytes: usize,
) -> Result<(), EnvironmentError> {
    let length = output
        .len()
        .checked_add(bytes.len())
        .ok_or(EnvironmentError::OutputLimit)?;
    if length > maximum_output_bytes {
        return Err(EnvironmentError::OutputLimit);
    }
    output.extend_from_slice(bytes);
    Ok(())
}

fn read_name(bytes: &[u8]) -> Option<(&[u8], usize)> {
    if bytes.first() == Some(&b'[') {
        let mut cursor = 1;
        let mut depth = 1_usize;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'[' => depth = depth.saturating_add(1),
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return (!bytes[1..cursor].is_empty())
                            .then_some((&bytes[1..cursor], cursor + 1));
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        return None;
    }
    if bytes.first() == Some(&b'(') {
        // A traditional two-character name may itself be a delayed string or
        // register reference, for example `\*(\*(Pi)`.  Consume the inner
        // reference atom as one name without recursive parsing; the existing
        // right-to-left resolver expands it later under its shared budget.
        if bytes.get(1) == Some(&b'\\') && matches!(bytes.get(2), Some(b'*' | b'n')) {
            let inner_length = reference_name_length(&bytes[3..])?;
            let end = 3_usize.checked_add(inner_length)?;
            return (end <= bytes.len()).then(|| (&bytes[1..end], end));
        }
        return (bytes.len() >= 3).then(|| (&bytes[1..3], 3));
    }
    // The compact form (`\nX` / `\*X`) names exactly one character;
    // the two-character spelling is explicitly introduced by `(`.
    (!bytes.is_empty()).then(|| (&bytes[..1], 1))
}

/// Return the byte length of one string/register name spelling without
/// executing it.  Bracket nesting is scanned iteratively, keeping nested
/// dynamic traditional names stack-safe.
fn reference_name_length(bytes: &[u8]) -> Option<usize> {
    match bytes.first().copied()? {
        b'[' => {
            let mut cursor = 1;
            let mut depth = 1_usize;
            while cursor < bytes.len() {
                match bytes[cursor] {
                    b'[' => depth = depth.saturating_add(1),
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(cursor + 1);
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
            None
        }
        b'(' => (bytes.len() >= 3).then_some(3),
        _ => Some(1),
    }
}

fn escaped_by_previous(bytes: &[u8], index: usize, escape: u8) -> bool {
    let mut count = 0_usize;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == escape {
        count += 1;
        cursor -= 1;
    }
    count % 2 == 1
}

fn contains_reference_escape(bytes: &[u8], escape: u8) -> bool {
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == escape
            && !escaped_by_previous(bytes, index, escape)
            && matches!(bytes.get(index + 1), Some(b'*' | b'n'))
    })
}

/// String names, unlike number-register names, apply literal delimiter
/// spellings while resolving their bracketed form.  Register names retain the
/// raw doubled spelling used by `.nr`/`.rr`, so the caller deliberately gates
/// this helper on the outer `\\*` reference kind.
fn contains_literal_string_name_escape(bytes: &[u8], escape: u8) -> bool {
    bytes.iter().enumerate().any(|(index, byte)| {
        *byte == escape
            && !escaped_by_previous(bytes, index, escape)
            && matches!(bytes.get(index + 1), Some(b'\\' | b'e'))
    })
}

#[cfg(test)]
mod tests {
    use crate::Limits;

    use super::{
        Environment, EnvironmentError, TranslationRequestIssue, translation_request_issue,
    };

    #[test]
    fn translation_validation_tracks_empty_and_odd_glyph_pairs() {
        assert_eq!(
            translation_request_issue(b"", b'\\'),
            Some(TranslationRequestIssue::Empty)
        );
        assert_eq!(
            translation_request_issue(b"x", b'\\'),
            Some(TranslationRequestIssue::Odd { start: 0, end: 1 })
        );
        assert_eq!(
            translation_request_issue(b"xy\\(em", b'\\'),
            Some(TranslationRequestIssue::Odd { start: 2, end: 6 })
        );
        assert_eq!(translation_request_issue(b"xy", b'\\'), None);
    }

    #[test]
    fn delayed_strings_registers_and_arguments_expand_without_global_state() {
        let mut environment = Environment::default();
        environment
            .define_string(b"name", b"mantdoc", false, &Limits::default())
            .unwrap();
        environment
            .define_register(b"count", b"42", None, &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"\\*[name] \\n[count] \\$1 \\n[.$]",
                b'\\',
                &[b"arg".to_vec()],
                4,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"mantdoc 42 arg 1");
        assert!(expansion.missing_references.is_empty());
        let mut empty = Environment::default();
        assert!(
            empty
                .expand(
                    b"\\*[name]",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes
                .is_empty()
        );
    }

    #[test]
    fn validates_numeric_and_width_escapes_before_visible_ast_normalization() {
        let mut environment = Environment::default();
        let expansion = environment
            .expand(
                b"\\B'1+2' \\B'1+' \\w'text' \\h'\\w'\\&'M'",
                b'\\',
                &[],
                8,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"1 0 96 \\h'0M'");
        assert_eq!(expansion.steps, 4);
    }

    #[test]
    fn unterminated_numeric_and_width_escapes_still_return_mandoc_recovery_values() {
        let mut environment = Environment::default();
        let numeric = environment
            .expand(
                b"\\B'1+1",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(numeric.bytes, b"0");
        assert_eq!(numeric.malformed_escape_offsets, vec![0]);

        let width = environment
            .expand(
                b"\\w'foo",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(width.bytes, b"72");
        assert_eq!(width.malformed_escape_offsets, vec![0]);
    }

    #[test]
    fn definitions_obey_shared_budgets_and_removal_reclaims_bytes() {
        let limits = Limits {
            max_definitions: 1,
            max_definition_bytes: 3,
            ..Limits::default()
        };
        let mut environment = Environment::default();
        environment
            .define_string(b"x", b"one", false, &limits)
            .unwrap();
        assert_eq!(
            environment.define_string(b"y", b"two", false, &limits),
            Err(EnvironmentError::DefinitionLimit)
        );
        environment.remove(b"x");
        environment
            .define_string(b"y", b"two", false, &limits)
            .unwrap();
    }

    #[test]
    fn macro_renaming_aliasing_and_relative_registers_are_session_local() {
        let mut environment = Environment::default();
        environment
            .define_macro(
                b"old",
                vec![b"value \\$1".to_vec()],
                false,
                &Limits::default(),
            )
            .unwrap();
        environment.rename(b"old", b"new");
        environment
            .alias_macro(b"new", b"copy", &Limits::default())
            .unwrap();
        assert!(environment.macro_definition(b"old").is_none());
        assert_eq!(
            environment.macro_definition(b"copy").unwrap().lines.len(),
            1
        );
        environment
            .define_register(b"n", b"2", None, &Limits::default())
            .unwrap();
        environment
            .define_register(b"n", b"+3", None, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\n[n]",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"5"
        );
    }

    #[test]
    fn traditional_register_names_auto_adjust_and_argument_lists_expand() {
        let mut environment = Environment::default();
        environment
            .define_register(b"co", b"2", Some(b"1"), &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"\\n-[co] \\n(co \\$* \\$@ \\n(.$",
                b'\\',
                &[b"one".to_vec(), b"two".to_vec()],
                8,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"1 1 one two one two 2");
    }

    #[test]
    fn compact_one_character_register_names_do_not_consume_following_text() {
        let mut environment = Environment::default();
        environment
            .define_register(b"Y", b"24", None, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\nY suffix",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"24 suffix"
        );
    }

    #[test]
    fn interpolating_an_undefined_register_materializes_its_zero_value() {
        let mut environment = Environment::default();
        assert!(!environment.is_register_defined(b"missing"));
        assert_eq!(
            environment
                .expand(
                    b"\\n[missing]",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"0"
        );
        assert!(environment.is_register_defined(b"missing"));
        environment.remove_register(b"missing");
        assert!(!environment.is_register_defined(b"missing"));
    }

    #[test]
    fn implicit_register_materialization_respects_the_session_definition_limit() {
        let limits = Limits {
            max_definitions: 0,
            ..Limits::default()
        };
        let mut environment = Environment::default();
        environment.configure_limits(&limits);
        assert_eq!(
            environment.expand(
                b"\\n[missing]",
                b'\\',
                &[],
                1,
                limits.max_expanded_line_bytes,
            ),
            Err(EnvironmentError::DefinitionLimit)
        );
    }

    #[test]
    fn predefined_registers_remain_read_only_and_have_legacy_values() {
        let mut environment = Environment::default();
        for name in [b".A".as_slice(), b".g", b".H", b".j", b".T", b".V"] {
            environment
                .define_register(name, b"111", None, &Limits::default())
                .unwrap();
            assert!(environment.is_register_defined(name));
        }
        assert_eq!(
            environment
                .expand(
                    b"\\n(.A \\n(.g \\n(.H \\n(.j \\n(.T \\n(.V",
                    b'\\',
                    &[],
                    6,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"0 1 24 0 1 40"
        );
    }

    #[test]
    fn number_register_arithmetic_wraps_at_the_legacy_i32_boundary() {
        let mut environment = Environment::default();
        environment
            .define_register(b"Y", b"2147483647", None, &Limits::default())
            .unwrap();
        environment
            .define_register(b"Y", b"+1", None, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\nY",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"-2147483648"
        );
    }

    #[test]
    fn nested_bracketed_names_expand_innermost_reference_first() {
        let mut environment = Environment::default();
        environment
            .define_string(b"foo", b"bar", false, &Limits::default())
            .unwrap();
        environment
            .define_string(b"bar", b"output", false, &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"\\*[\\*[foo]]",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"output");
        assert!(expansion.missing_references.is_empty());
    }

    #[test]
    fn predefined_strings_are_lazy_and_user_overridable() {
        let mut environment = Environment::default();
        let expansion = environment
            .expand(
                b"\\*[Pi] \\*(Pi",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"pi pi");
        assert!(expansion.missing_references.is_empty());
        assert!(environment.is_name_defined(b"Pi"));
        assert_eq!(
            environment
                .expand(
                    b"\\*(.T \\*[.T]",
                    b'\\',
                    &[],
                    2,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"\\*(.T \\*[.T]"
        );
        assert!(environment.is_name_defined(b".T"));

        environment
            .define_string(b"Pi", b"override", false, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\*[Pi]",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"override"
        );
        environment
            .define_string(b".T", b"named", false, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\*(.T",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"named"
        );
    }

    #[test]
    fn macro_names_interpolate_as_silent_zero_length_strings() {
        let mut environment = Environment::default();
        environment
            .define_macro(b"empty", Vec::new(), false, &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"before\\*[empty]after",
                b'\\',
                &[],
                1,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"beforeafter");
        assert!(expansion.missing_references.is_empty());
    }

    #[test]
    fn traditional_dynamic_names_expand_without_recursive_name_parsing() {
        let mut environment = Environment::default();
        environment
            .define_string(b"pi", b"surprising", false, &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"\\*(\\*(Pi",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"surprising");
        assert!(expansion.missing_references.is_empty());
    }

    #[test]
    fn literal_copy_mode_escapes_in_a_name_are_not_dynamic_references() {
        let mut environment = Environment::default();
        environment
            .define_string(b"std\\esc", b"stdval", false, &Limits::default())
            .unwrap();
        let expansion = environment
            .expand(
                b"\\*[std\\\\esc]",
                b'\\',
                &[],
                1,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"stdval");
        assert!(expansion.missing_references.is_empty());
    }

    #[test]
    fn number_register_steps_default_to_zero_and_survive_value_redefinition() {
        let mut environment = Environment::default();
        environment
            .define_register(b"n", b"2", None, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\n-[n] \\n+[n]",
                    b'\\',
                    &[],
                    2,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"2 2"
        );
        environment
            .define_register(b"n", b"0", Some(b"3"), &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\n+[n] \\n+[n]",
                    b'\\',
                    &[],
                    2,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"3 6"
        );
        environment
            .define_register(b"n", b"42", None, &Limits::default())
            .unwrap();
        assert_eq!(
            environment
                .expand(
                    b"\\n-[n]",
                    b'\\',
                    &[],
                    1,
                    Limits::default().max_expanded_line_bytes,
                )
                .unwrap()
                .bytes,
            b"39"
        );
    }

    #[test]
    fn renaming_over_a_definition_reclaims_the_replaced_storage() {
        let limits = Limits {
            max_definition_bytes: 6,
            ..Limits::default()
        };
        let mut environment = Environment::default();
        environment
            .define_string(b"old", b"one", false, &limits)
            .unwrap();
        environment
            .define_string(b"new", b"two", false, &limits)
            .unwrap();
        environment.rename(b"old", b"new");
        environment
            .define_string(b"old", b"two", false, &limits)
            .unwrap();
    }

    #[test]
    fn removing_a_register_does_not_remove_a_same_named_string() {
        let mut environment = Environment::default();
        environment
            .define_string(b"value", b"string", false, &Limits::default())
            .unwrap();
        environment
            .define_register(b"value", b"7", None, &Limits::default())
            .unwrap();
        environment.remove_register(b"value");
        let expansion = environment
            .expand(
                b"\\*[value] \\n[value]",
                b'\\',
                &[],
                2,
                Limits::default().max_expanded_line_bytes,
            )
            .unwrap();
        assert_eq!(expansion.bytes, b"string 0");
        assert!(expansion.missing_references.is_empty());
    }

    #[test]
    fn environment_expansion_stops_at_the_explicit_work_budget() {
        let mut environment = Environment::default();
        environment
            .define_string(b"x", b"x", false, &Limits::default())
            .unwrap();
        assert_eq!(
            environment.expand(
                b"\\*[x]\\*[x]",
                b'\\',
                &[],
                1,
                Limits::default().max_expanded_line_bytes,
            ),
            Err(EnvironmentError::ExpansionLimit)
        );
    }

    #[test]
    fn cyclic_string_expansion_is_bounded_before_the_host_stack() {
        let mut environment = Environment::default();
        environment
            .define_string(b"cycle", b"\\*[cycle]", false, &Limits::default())
            .unwrap();
        assert_eq!(
            environment.expand(
                b"\\*[cycle]",
                b'\\',
                &[],
                usize::MAX,
                Limits::default().max_expanded_line_bytes,
            ),
            Err(EnvironmentError::RecursionLimit)
        );
    }

    #[test]
    fn environment_expansion_stops_before_allocating_past_output_budget() {
        let mut environment = Environment::default();
        environment
            .define_string(b"x", b"four", false, &Limits::default())
            .unwrap();
        assert_eq!(
            environment.expand(b"\\*[x]", b'\\', &[], 1, 3),
            Err(EnvironmentError::OutputLimit)
        );
    }
}
