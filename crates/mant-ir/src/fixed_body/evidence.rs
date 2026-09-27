//! Read-time evidence closure for native owner heads and text selections.
//!
//! This module validates surviving glyph ranges, declaration syntax and
//! bindings against one Fixed surface. It neither formats roff nor owns the
//! native collector state; every proof is recomputed for mutable documents.

use super::literal_boundaries::literal_name_starts;
use super::{
    FixedBody, OutputSlice, OwnerHeadComponent, OwnerHeadRole, OwnerMark, OwnerRole, RegionKind,
    TextJoin, TextSelection, validation,
};
use crate::{EntryFacts, EntryKind, EntryNameEvidence, NameCase, ParameterKind};
use std::{collections::BTreeSet, num::NonZeroU32, ops::Range};

/// Map each native component to a contiguous HEAD part interval. A component
/// may leave unrelated HEAD glyphs outside its interval, but cannot skip a
/// part or substitute a different join inside its own visible spelling.
pub(super) fn component_part_ranges(
    head: &TextSelection,
    components: &[OwnerHeadComponent],
) -> Option<Vec<Range<usize>>> {
    let mut ranges = Vec::with_capacity(components.len());
    let mut cursor = 0;
    for component in components {
        let first = component.selection.parts.first()?;
        while head.parts.get(cursor).is_some_and(|part| part != first) {
            cursor += 1;
        }
        let end = cursor.checked_add(component.selection.parts.len())?;
        if head.parts.get(cursor..end)? != component.selection.parts
            || head.joins.get(cursor..end.saturating_sub(1))? != component.selection.joins
        {
            return None;
        }
        ranges.push(cursor..end);
        cursor = end;
    }
    Some(ranges)
}

fn group_name_occurrences(
    found: impl IntoIterator<Item = (String, TextSelection)>,
) -> Vec<(String, Vec<TextSelection>)> {
    let mut indices: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut grouped: Vec<(String, Vec<TextSelection>)> = Vec::new();
    for (name, selection) in found {
        if let Some(&index) = indices.get(&name) {
            grouped[index].1.push(selection);
        } else {
            indices.insert(name.clone(), grouped.len());
            grouped.push((name, vec![selection]));
        }
    }
    grouped
}

/// One complete non-option declaration over the owner's surviving HEAD.
/// Native roles and source identity are checked before these byte ranges are
/// exposed; the producer and read-time validator consume this same result.
#[doc(hidden)]
pub struct FixedNonOptionRecognition {
    pub kind: EntryKind,
    pub evidence: EntryNameEvidence,
    pub occurrences: Vec<(String, TextSelection)>,
}

/// One native Ic/Cm prefix after the same base name grammar used by a
/// complete Literal declaration, before parent context changes its category.
struct LiteralPrefixProof<'a> {
    base_kind: EntryKind,
    name_kind: EntryKind,
    name: String,
    selection: &'a TextSelection,
}

/// Bounded name proof was omitted, not syntactically disproved. The producer
/// must publish a semantic-coverage diagnostic while retaining the display.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedNonOptionLimit {
    TooManyNames,
    TooManyComponents,
}

/// One immutable owner pass. Parent proofs are evaluated once, before their
/// children, and are never stored in the document or trusted after mutation.
#[doc(hidden)]
pub struct FixedEntryPass<'a> {
    body: &'a FixedBody,
    nested: Vec<bool>,
}

impl<'a> FixedEntryPass<'a> {
    fn new(body: &'a FixedBody) -> Self {
        let count = body.owners.len();
        let mut has_child = vec![false; count];
        for owner in &body.owners {
            if let Some(parent) = owner.parent
                && let Some(slot) = has_child.get_mut((parent.get() - 1) as usize)
            {
                *slot = true;
            }
        }
        let mut nested = vec![false; count];
        let mut establishes = vec![false; count];
        let mut depth = vec![0_u8; count];
        for (index, owner) in body.owners.iter().enumerate() {
            if let Some(parent) = owner.parent {
                let parent = (parent.get() - 1) as usize;
                if parent >= index {
                    nested[index] = true;
                    depth[index] = 65;
                } else {
                    depth[index] = depth[parent].saturating_add(1);
                    nested[index] = nested[parent] || establishes[parent] || depth[index] > 64;
                }
            }
            if has_child[index] {
                establishes[index] = body.owner_establishes_semantic_context(owner);
            }
        }
        Self { body, nested }
    }

    fn nested_for(&self, owner: &OwnerMark) -> bool {
        let index = (owner.key.get() - 1) as usize;
        self.body
            .owners
            .get(index)
            .is_none_or(|stored| !std::ptr::eq(stored, owner))
            || self.nested.get(index).copied().unwrap_or(true)
    }

    /// Reuse this pass's checked ancestry for one native manual-call name.
    #[must_use]
    pub fn manual_call_declaration(&self, owner: &OwnerMark) -> Option<FixedNonOptionRecognition> {
        self.body
            .manual_call_declaration_with_context(owner, self.nested_for(owner))
    }

    /// Reuse checked ancestry for one complete non-option HEAD.
    ///
    /// # Errors
    /// Returns a semantic budget limit without hiding the native text.
    pub fn scan_non_option_declaration(
        &self,
        owner: &OwnerMark,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        self.body
            .scan_non_option_declaration_with_context(owner, self.nested_for(owner))
    }

    /// Bind a partial native name using the same base syntax as a complete
    /// declaration, then apply the current parent category.
    #[must_use]
    pub fn partial_literal_declaration(
        &self,
        owner: &OwnerMark,
    ) -> Option<FixedNonOptionRecognition> {
        self.body
            .partial_literal_declaration_with_context(owner, self.nested_for(owner))
    }

    /// Keep a partial native category while using the current parent context.
    #[must_use]
    pub fn partial_literal_entry_kind(&self, owner: &OwnerMark) -> EntryKind {
        self.body
            .partial_literal_entry_kind_with_context(owner, self.nested_for(owner))
    }

    /// Bind one visible native prefix to its checked category.
    #[must_use]
    pub fn native_head_identity(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<(EntryKind, EntryNameEvidence, String, TextSelection)> {
        self.body
            .native_head_identity_with_context(owner, form, self.nested_for(owner))
    }

    /// Validate one owner's facts against this immutable operation snapshot.
    #[must_use]
    pub(crate) fn validated_entry<'b>(
        &self,
        owner: &'b OwnerMark,
    ) -> Option<&'b EntryFacts<TextSelection>> {
        let index = (owner.key.get() - 1) as usize;
        if !self
            .body
            .owners
            .get(index)
            .is_some_and(|stored| std::ptr::eq(stored, owner))
        {
            return None;
        }
        self.body
            .validated_entry_with_context(owner, self.nested_for(owner))
    }

    pub(crate) const fn body(&self) -> &'a FixedBody {
        self.body
    }
}

impl FixedBody {
    /// Build a transient parent-first semantic proof for one immutable pass.
    #[doc(hidden)]
    #[must_use]
    pub fn entry_pass(&self) -> FixedEntryPass<'_> {
        FixedEntryPass::new(self)
    }
    /// Prove a non-option declaration once over the full native HEAD. Explicit
    /// Ev/Va/Dv components have stronger evidence than a lexical ENVIRONMENT
    /// head; neither font appearance nor raw roff spelling supplies a name.
    #[must_use]
    pub fn non_option_declaration(&self, owner: &OwnerMark) -> Option<FixedNonOptionRecognition> {
        self.scan_non_option_declaration(owner).ok().flatten()
    }

    /// The producer uses this variant to distinguish a semantic name budget
    /// from an ordinary unrecognized HEAD; validation uses the same proof.
    ///
    /// # Errors
    ///
    /// Returns `TooManyNames` if a declaration exceeds the semantic
    /// occurrence budget; the native display remains valid.
    pub fn scan_non_option_declaration(
        &self,
        owner: &OwnerMark,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        self.scan_non_option_declaration_with_context(
            owner,
            self.nested_under_semantic_parent(owner),
        )
    }

    fn scan_non_option_declaration_with_context(
        &self,
        owner: &OwnerMark,
        nested: bool,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        if owner.role != OwnerRole::Definition {
            return Ok(None);
        }
        if owner.hanging_candidate {
            return if self.hanging_structure_ready(owner) {
                self.contextual_lexical_non_option_declaration(owner, true, nested)
            } else {
                Ok(None)
            };
        }
        let Some(head_role) = owner.head_role else {
            return Ok(None);
        };
        match head_role {
            OwnerHeadRole::Lexical => {
                self.contextual_lexical_non_option_declaration(owner, false, nested)
            }
            OwnerHeadRole::Literal => self.native_literal_declaration_with_context(owner, nested),
            OwnerHeadRole::Option
            | OwnerHeadRole::Environment
            | OwnerHeadRole::Variable
            | OwnerHeadRole::DefinedVariable => self.native_non_option_declaration(owner),
            OwnerHeadRole::Argument => Ok(None),
        }
    }

    fn section_is_environment(&self, owner: &OwnerMark) -> bool {
        self.section_declaration_family(owner)
            == Some(crate::SectionDeclarationFamily::EnvironmentVariables)
    }

    /// An unrecognized lexical label in ENVIRONMENT stays a physical owner,
    /// not a substitute Term. A template such as `FILE_TEMPLATE_*` has no
    /// exact environment-variable selector.
    #[doc(hidden)]
    #[must_use]
    pub fn unbound_environment_head(&self, owner: &OwnerMark) -> bool {
        (owner.head_role == Some(OwnerHeadRole::Lexical)
            || self
                .owner_complete_form(owner)
                .is_some_and(|form| crate::is_environment_template_label(&form)))
            && self.section_is_environment(owner)
    }

    /// A malformed variable-like TP/IP label cannot become a complete-name
    /// Term through the generic fallback. The physical HEAD/BODY and native
    /// display remain available without a semantic selector.
    #[doc(hidden)]
    #[must_use]
    pub fn unbound_variable_head(&self, owner: &OwnerMark) -> bool {
        if owner.role != OwnerRole::Definition
            || !matches!(owner.head_role, None | Some(OwnerHeadRole::Lexical))
            || owner.hanging_candidate
            || self.section_declaration_family(owner)
                != Some(crate::SectionDeclarationFamily::Variables)
        {
            return false;
        }
        self.owner_complete_form(owner)
            .is_some_and(|form| crate::rejected_variable_declaration_head(&form))
    }

    fn section_is_commands(&self, owner: &OwnerMark) -> bool {
        self.section_declaration_family(owner) == Some(crate::SectionDeclarationFamily::Commands)
    }

    /// Bind a complete lexical declaration to the same surviving HEAD in
    /// either a real TP/IP definition or a structurally checked PP/RS pair.
    /// Weak pairs require extra local syntax; `man_term.c::pre_PP/pre_RS` only
    /// execute paragraph spacing and indentation, while `pre_TP` executes an
    /// explicit label HEAD. Classification here is `ManT` policy, not upstream.
    fn lexical_non_option_declaration(
        &self,
        owner: &OwnerMark,
        hanging: bool,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return Ok(None);
        }
        let Some(form) = self.owner_complete_form(owner) else {
            return Ok(None);
        };
        let (kind, names) = match self.section_declaration_family(owner) {
            Some(crate::SectionDeclarationFamily::EnvironmentVariables) => {
                let names = crate::scan_environment_declaration_names(&form)
                    .map_err(|_| FixedNonOptionLimit::TooManyNames)?;
                let Some(names) = names.filter(|names| !names.is_empty()) else {
                    return Ok(None);
                };
                // A weak PP/RS paragraph needs a complete assignment or an
                // independently delimited name group. Bare GIT_DIR remains
                // visible prose; an explicit TP/IP label has its own boundary.
                let complete_assignment = form
                    .split_once('=')
                    .is_some_and(|(_, value)| !value.trim().is_empty());
                if hanging && names.len() < 2 && !complete_assignment && !form.contains('<') {
                    return Ok(None);
                }
                (EntryKind::EnvironmentVariable, names)
            }
            Some(crate::SectionDeclarationFamily::Variables) => {
                let range = if hanging {
                    crate::variable_assignment_declaration_range(&form)
                } else {
                    crate::variable_declaration_name_range(&form)
                };
                let Some(range) = range else {
                    return Ok(None);
                };
                let Some(name) = form.get(range.clone()) else {
                    return Ok(None);
                };
                (EntryKind::Variable, vec![(name.to_owned(), range)])
            }
            Some(crate::SectionDeclarationFamily::ConfigurationKeys) => {
                if hanging && !form.contains(['.', '=']) {
                    return Ok(None);
                }
                let Some(range) = crate::configuration_key_declaration_range(&form) else {
                    return Ok(None);
                };
                let Some(name) = form.get(range.clone()) else {
                    return Ok(None);
                };
                (EntryKind::ConfigurationKey, vec![(name.to_owned(), range)])
            }
            Some(crate::SectionDeclarationFamily::Commands) => {
                let Some(range) = crate::command_declaration_name_range(&form) else {
                    return Ok(None);
                };
                let Some(name) = form.get(range.clone()) else {
                    return Ok(None);
                };
                (EntryKind::Command, vec![(name.to_owned(), range)])
            }
            _ if self.root_configuration_hint && self.root_configuration_scope(owner) => {
                let Some(range) = crate::root_configuration_assignment_range(&form) else {
                    return Ok(None);
                };
                let Some(name) = form.get(range.clone()) else {
                    return Ok(None);
                };
                (EntryKind::ConfigurationKey, vec![(name.to_owned(), range)])
            }
            _ => return Ok(None),
        };
        let ranges = names
            .iter()
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        let Some(selections) = self.selection_subranges_from_form(&owner.head, &form, &ranges)
        else {
            return Ok(None);
        };
        let occurrences = names
            .into_iter()
            .zip(selections)
            .filter_map(|((name, _), selection)| {
                (self.selection_text(&selection).as_deref() == Some(name.as_str()))
                    .then_some((name, selection))
            })
            .collect::<Vec<_>>();
        if occurrences.len() != ranges.len() {
            return Ok(None);
        }
        Ok(Some(FixedNonOptionRecognition {
            kind,
            evidence: EntryNameEvidence::Lexical,
            occurrences,
        }))
    }

    fn contextual_lexical_non_option_declaration(
        &self,
        owner: &OwnerMark,
        hanging: bool,
        nested: bool,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        // A semantic parent changes the weak section-derived category, not
        // the independently checked name or its surviving display range.
        let mut recognition = self.lexical_non_option_declaration(owner, hanging)?;
        if nested
            && let Some(recognition) = &mut recognition
            && matches!(
                recognition.kind,
                EntryKind::Command | EntryKind::ConfigurationKey
            )
        {
            recognition.kind = EntryKind::Term;
        }
        Ok(recognition)
    }

    fn section_declaration_family(
        &self,
        owner: &OwnerMark,
    ) -> Option<crate::SectionDeclarationFamily> {
        let mut section = owner.section;
        while let Some(key) = section {
            let heading = self.headings.get((key.get() - 1) as usize)?;
            let title = self.selection_text(&heading.title)?;
            // `NonDeclaration` is a scope barrier, not an unknown heading:
            // stop here rather than finding an ancestor COMMANDS/OPTIONS.
            // man_macro.c::rew_scope preserves the SH/SS nesting that this
            // nearest-heading walk follows.
            if let Some(family) = crate::section_declaration_family(&title) {
                return Some(family);
            }
            section = heading.parent;
        }
        None
    }

    /// Ic and Cm currently share the native Literal role. The nearest
    /// declaration section and a checked root configuration-manual hint
    /// select a *weak* type; neither font nor macro spelling does so.
    #[doc(hidden)]
    #[must_use]
    pub fn literal_entry_kind(&self, owner: &OwnerMark) -> EntryKind {
        self.literal_entry_kind_with_context(owner, self.nested_under_semantic_parent(owner))
    }

    fn literal_entry_kind_with_context(&self, owner: &OwnerMark, nested: bool) -> EntryKind {
        if nested {
            // Options, keys and commands put their child definitions in value
            // or parameter context. No checked Value subtype exists here, so
            // keep independently bound native names as Terms.
            return EntryKind::Term;
        }
        self.base_literal_entry_kind(owner)
    }

    fn base_literal_entry_kind(&self, owner: &OwnerMark) -> EntryKind {
        match self.section_declaration_family(owner) {
            Some(crate::SectionDeclarationFamily::Commands) => EntryKind::Command,
            Some(crate::SectionDeclarationFamily::ConfigurationKeys) => EntryKind::ConfigurationKey,
            Some(crate::SectionDeclarationFamily::NonDeclaration) | None
                if self.root_configuration_hint && self.root_configuration_scope(owner) =>
            {
                EntryKind::ConfigurationKey
            }
            // Prose/reference scopes block inherited command context, but
            // cannot erase a call proved within this very definition head.
            Some(crate::SectionDeclarationFamily::NonDeclaration) | None
                if self.local_literal_command_call(owner) =>
            {
                EntryKind::Command
            }
            Some(
                crate::SectionDeclarationFamily::Parameters
                | crate::SectionDeclarationFamily::EnvironmentVariables
                | crate::SectionDeclarationFamily::Variables
                | crate::SectionDeclarationFamily::NonDeclaration,
            )
            | None => EntryKind::Term,
        }
    }

    /// A topical mdoc section can contain a command catalogue without a
    /// heading literally named COMMANDS. Require the *same* complete It HEAD
    /// to prove a delimited Ic/Cm name and an authored Fl option inside the
    /// visible call signature. `mdoc_macro.c::blk_full/blk_part_exp` keep the
    /// Xo/Xc call in one HEAD; `mdoc_term.c::termp_fl_pre` writes the option.
    /// A prior sibling heading, font alone, or an Op Ar value proves nothing.
    fn local_literal_command_call(&self, owner: &OwnerMark) -> bool {
        if owner.head_components.len() > 4096 {
            return false;
        }
        let Some((name, _)) = self.literal_command_component(owner) else {
            return false;
        };
        let Some(form) = self.owner_complete_form(owner) else {
            return false;
        };
        if !form.starts_with(&name) {
            return false;
        }
        let Some(part_ranges) = component_part_ranges(&owner.head, &owner.head_components) else {
            return false;
        };
        let Some(byte_ranges) = self.component_byte_ranges(&owner.head, &part_ranges) else {
            return false;
        };
        owner
            .head_components
            .iter()
            .zip(byte_ranges)
            .skip(1)
            .any(|(component, range)| {
                component.role == OwnerHeadRole::Option
                    && component.has_source_identity()
                    && range.start > name.len()
                    && form
                        .get(..range.start)
                        .is_some_and(|prefix| prefix.ends_with(" ["))
                    && self
                        .selection_text(&component.selection)
                        .is_some_and(|text| {
                            crate::native_option_token(&text)
                                && form.get(range.clone()) == Some(text.as_str())
                        })
            })
    }

    fn nested_under_semantic_parent(&self, owner: &OwnerMark) -> bool {
        let mut parent = owner.parent;
        // Single-owner callers use this bounded proof; whole-owner passes
        // use FixedEntryPass to prove each relevant parent only once.
        for _ in 0..64 {
            let Some(key) = parent else {
                return false;
            };
            let Some(ancestor) = self.owners.get((key.get() - 1) as usize) else {
                return true;
            };
            if self.owner_establishes_semantic_context(ancestor) {
                return true;
            }
            parent = ancestor.parent;
        }
        parent.is_some()
    }

    fn owner_establishes_semantic_context(&self, owner: &OwnerMark) -> bool {
        if owner.head_role == Some(OwnerHeadRole::Option) {
            return true;
        }
        if owner.role != OwnerRole::Definition {
            return false;
        }
        let hanging_ready = !owner.hanging_candidate || self.hanging_structure_ready(owner);
        if owner.head_role == Some(OwnerHeadRole::Lexical)
            && hanging_ready
            && self
                .lexical_names(owner)
                .is_some_and(|names| !names.is_empty())
        {
            return true;
        }
        if owner.head_role == Some(OwnerHeadRole::Literal) {
            let base = self.base_literal_entry_kind(owner);
            if matches!(base, EntryKind::Command | EntryKind::ConfigurationKey)
                && self
                    .native_literal_declaration_for_kind(owner, base)
                    .ok()
                    .flatten()
                    .is_some_and(|recognition| recognition.kind == base)
            {
                return true;
            }
        }
        self.manual_call_declaration_base(owner).is_some()
            || owner.head_role == Some(OwnerHeadRole::Lexical)
                && hanging_ready
                && self
                    .lexical_non_option_declaration(owner, owner.hanging_candidate)
                    .ok()
                    .flatten()
                    .is_some_and(|recognition| {
                        matches!(
                            recognition.kind,
                            EntryKind::Command | EntryKind::ConfigurationKey
                        )
                    })
    }

    /// An Xo HEAD with an unknown later join cannot prove a complete key.
    /// Preserve the literal name as a Term until its full declaration closes.
    #[doc(hidden)]
    #[must_use]
    pub fn partial_literal_entry_kind(&self, owner: &OwnerMark) -> EntryKind {
        self.partial_literal_entry_kind_with_context(
            owner,
            self.nested_under_semantic_parent(owner),
        )
    }

    fn partial_literal_entry_kind_with_context(
        &self,
        owner: &OwnerMark,
        nested: bool,
    ) -> EntryKind {
        match self.literal_entry_kind_with_context(owner, nested) {
            EntryKind::ConfigurationKey => EntryKind::Term,
            kind => kind,
        }
    }

    fn partial_literal_declaration_with_context(
        &self,
        owner: &OwnerMark,
        nested: bool,
    ) -> Option<FixedNonOptionRecognition> {
        let prefix = self.literal_prefix_proof(owner)?;
        let kind = if prefix.name_kind == EntryKind::Term {
            EntryKind::Term
        } else {
            self.partial_literal_entry_kind_with_context(owner, nested)
        };
        Some(FixedNonOptionRecognition {
            kind,
            evidence: EntryNameEvidence::NativeMarkup,
            occurrences: vec![(prefix.name, prefix.selection.clone())],
        })
    }

    /// A root config manual can declare keys in DESCRIPTION, but a SEE ALSO
    /// or EXAMPLES section is still a prose barrier. No prior sibling entry
    /// can make a later owner into a key.
    fn root_configuration_scope(&self, owner: &OwnerMark) -> bool {
        let mut section = owner.section;
        // Follow every native Sh/Ss ancestor, not only the nearest recognized
        // title. A nested DESCRIPTION cannot reopen an EXAMPLES/SEE ALSO
        // barrier; mdoc_macro.c::blk_full nests Ss under its Sh, while
        // mdoc_term.c::termp_sh_pre/termp_ss_pre only format the headings.
        for _ in 0..self.headings.len() {
            let Some(key) = section else {
                return true;
            };
            let Some(heading) = self.headings.get((key.get() - 1) as usize) else {
                return false;
            };
            let Some(title) = self.selection_text(&heading.title) else {
                return false;
            };
            if let Some(family) = crate::section_declaration_family(&title)
                && (family != crate::SectionDeclarationFamily::NonDeclaration
                    || !title.trim().eq_ignore_ascii_case("DESCRIPTION"))
            {
                return false;
            }
            section = heading.parent;
        }
        // An impossible cycle or overlong ancestry is not a proof. Unknown
        // headings otherwise inherit the root hint; DESCRIPTION preserves it.
        section.is_none()
    }

    /// True only for a key whose classification needs the document-level
    /// hint. Document indexing can withhold such facts if metadata mutates.
    pub(crate) fn root_configuration_dependent(&self, owner: &OwnerMark) -> bool {
        owner
            .entry
            .as_ref()
            .is_some_and(|entry| entry.kind == EntryKind::ConfigurationKey)
            && self.section_declaration_family(owner)
                != Some(crate::SectionDeclarationFamily::ConfigurationKeys)
    }

    fn literal_name_range(text: &str, kind: EntryKind) -> Option<Range<usize>> {
        match kind {
            EntryKind::ConfigurationKey => crate::configuration_key_declaration_range(text),
            EntryKind::Command | EntryKind::Term if crate::native_command_token(text) => {
                Some(0..text.len())
            }
            _ => None,
        }
    }

    /// A literal Cm/Ic dash is an authored modifier name, unlike a generated
    /// list bullet or a plain TP marker. Require one surviving, source-bound
    /// native instance covering the entire visible HEAD; font alone is not
    /// sufficient. `mdoc_term.c::termp_it_pre` keeps the tag HEAD distinct from
    /// bullet/enum items, and `termp_bold_pre` executes Cm/Ic's glyph.
    fn literal_dash_recognition(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<FixedNonOptionRecognition> {
        if owner.role != OwnerRole::Definition
            || owner.head_role != Some(OwnerHeadRole::Literal)
            || owner.hanging_candidate
            || !matches!(form, "-" | "--")
        {
            return None;
        }
        let [component] = owner.head_components.as_slice() else {
            return None;
        };
        if component.role != OwnerHeadRole::Literal
            || !component.has_source_identity()
            || component.selection != owner.head
            || self.selection_text(&component.selection).as_deref() != Some(form)
        {
            return None;
        }
        Some(FixedNonOptionRecognition {
            kind: EntryKind::Term,
            evidence: EntryNameEvidence::NativeMarkup,
            occurrences: vec![(form.to_owned(), component.selection.clone())],
        })
    }

    /// A complete literal HEAD may contain several independently executed
    /// Ic/Cm instances. Only parameter-external separators can restart a
    /// declaration; instances inside one Ar enclosure remain visible but
    /// cannot become independent names.
    fn native_literal_declaration_with_context(
        &self,
        owner: &OwnerMark,
        nested: bool,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        // Configuration keys can carry a value suffix in the same Cm
        // instance. Bind their visible name with the section's base syntax
        // before parent context weakens the category; Term syntax would
        // otherwise reinterpret `child=value` as one different name.
        let base = self.base_literal_entry_kind(owner);
        let mut recognition = self.native_literal_declaration_for_kind(owner, base)?;
        if nested
            && let Some(recognition) = &mut recognition
            && matches!(
                recognition.kind,
                EntryKind::Command | EntryKind::ConfigurationKey
            )
        {
            recognition.kind = EntryKind::Term;
        }
        Ok(recognition)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one bounded native Literal component pass keeps the name and argument state together"
    )]
    fn native_literal_declaration_for_kind(
        &self,
        owner: &OwnerMark,
        requested_kind: EntryKind,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        if owner
            .head_components
            .iter()
            .filter(|component| component.role == OwnerHeadRole::Literal)
            .take(65)
            .count()
            > 64
        {
            return Err(FixedNonOptionLimit::TooManyNames);
        }
        // Budget the evidence itself before attempting to map its rendered
        // ranges. A malformed or unavailable selection cannot hide the work
        // required to inspect an overlong sequence of authored Ar instances.
        if owner
            .head_components
            .iter()
            .filter(|component| component.role == OwnerHeadRole::Argument)
            .take(65)
            .count()
            > 64
        {
            return Err(FixedNonOptionLimit::TooManyComponents);
        }
        if owner.head_components.len() > 4096 {
            return Err(FixedNonOptionLimit::TooManyComponents);
        }
        let Some(form) = self.owner_complete_form(owner) else {
            return Ok(None);
        };
        if let Some(marker) = self.literal_dash_recognition(owner, &form) {
            return Ok(Some(marker));
        }
        // ENVIRONMENT templates name a family, not one selectable variable
        // or a fallback Term. Check the single materialized HEAD before its
        // Literal components: an authored Cm instance cannot instantiate `*`.
        if self.section_is_environment(owner) && crate::is_environment_template_label(&form) {
            return Ok(None);
        }
        let Some(part_ranges) = component_part_ranges(&owner.head, &owner.head_components) else {
            return Ok(None);
        };
        let Some(byte_ranges) = self.component_byte_ranges(&owner.head, &part_ranges) else {
            return Ok(None);
        };
        let Some(name_starts) = literal_name_starts(&form, &owner.head_components, &byte_ranges)
        else {
            return Ok(None);
        };
        let mut kind = requested_kind;
        let mut names = Vec::with_capacity(byte_ranges.len());
        let mut previous_end = 0;
        let mut current_bare_name = false;
        let mut argument_count = 0usize;
        for ((component, range), starts_name) in owner
            .head_components
            .iter()
            .zip(byte_ranges)
            .zip(name_starts)
        {
            if !component.has_source_identity() || range.start >= range.end {
                return Ok(None);
            }
            let (Some(gap), Some(visible)) =
                (form.get(previous_end..range.start), form.get(range.clone()))
            else {
                return Ok(None);
            };
            match component.role {
                OwnerHeadRole::Literal => {
                    if !starts_name {
                        // A Cm/Ic instance inside an authored Ar quote or
                        // bracket is still an argument. Retain its display
                        // and scan the rest of the HEAD for a later, real
                        // separator rather than rejecting all earlier names.
                        if names.is_empty() || !current_bare_name {
                            return Ok(None);
                        }
                        previous_end = range.end;
                        continue;
                    }
                    if (names.is_empty() && !gap.trim().is_empty())
                        || (!names.is_empty() && !crate::complete_literal_component_gap(gap))
                    {
                        return Ok(None);
                    }
                    // Bind each independently proved native name before
                    // deciding its weak section-derived category. An `@` in
                    // one child name cannot erase its sibling's binding just
                    // because the pair is not a configuration-key group.
                    let Some(name_range) = Self::literal_name_range(visible, requested_kind)
                        .or_else(|| {
                            (requested_kind == EntryKind::ConfigurationKey)
                                .then(|| Self::literal_name_range(visible, EntryKind::Term))
                                .flatten()
                                .inspect(|_| kind = EntryKind::Term)
                        })
                    else {
                        return Ok(None);
                    };
                    let absolute = range.start + name_range.start..range.start + name_range.end;
                    let Some(name) = form.get(absolute.clone()) else {
                        return Ok(None);
                    };
                    names.push((name.to_owned(), absolute));
                    current_bare_name = name_range.end == visible.len();
                    argument_count = 0;
                }
                OwnerHeadRole::Argument => {
                    // Ar and Em render with the same underline font in
                    // mdoc_term.c. Only this checked native Ar component can
                    // license a parameter after a complete literal name.
                    if argument_count == 64 {
                        return Err(FixedNonOptionLimit::TooManyComponents);
                    }
                    if names.is_empty()
                        || !current_bare_name
                        || gap.is_empty()
                        || !gap.chars().all(char::is_whitespace)
                        || !crate::native_argument_component_token(visible)
                    {
                        return Ok(None);
                    }
                    argument_count += 1;
                }
                _ => return Ok(None),
            }
            previous_end = range.end;
        }
        if names.is_empty()
            || !form
                .get(previous_end..)
                .is_some_and(|suffix| suffix.trim().is_empty())
        {
            return Ok(None);
        }
        let ranges = names
            .iter()
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        let Some(selections) = self.selection_subranges_from_form(&owner.head, &form, &ranges)
        else {
            return Ok(None);
        };
        Ok(Some(FixedNonOptionRecognition {
            kind,
            evidence: EntryNameEvidence::NativeMarkup,
            occurrences: names
                .into_iter()
                .zip(selections)
                .map(|((name, _), selection)| (name, selection))
                .collect(),
        }))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one bounded Ev/Va/Dv component pass validates roles, gaps and surviving name ranges"
    )]
    fn native_non_option_declaration(
        &self,
        owner: &OwnerMark,
    ) -> Result<Option<FixedNonOptionRecognition>, FixedNonOptionLimit> {
        // The owner's first authored role can be a zero-glyph instance. The
        // typed IR intentionally retains only surviving component selections;
        // classify from the first visible instance and require every other
        // visible instance to agree. An empty Va before an Ev is not a Va
        // name, and a visible Va mixed with Ev is not one category.
        let Some(role) = owner
            .head_components
            .first()
            .map(|component| component.role)
        else {
            return Ok(None);
        };
        if owner.head_role != Some(role) {
            return Ok(None);
        }
        let kind = match role {
            OwnerHeadRole::Environment => EntryKind::EnvironmentVariable,
            OwnerHeadRole::Variable => EntryKind::Variable,
            OwnerHeadRole::DefinedVariable => EntryKind::Term,
            OwnerHeadRole::Option
            | OwnerHeadRole::Lexical
            | OwnerHeadRole::Literal
            | OwnerHeadRole::Argument => {
                return Ok(None);
            }
        };
        if owner.head_components.len() > 64 {
            return Err(FixedNonOptionLimit::TooManyNames);
        }
        let Some(form) = self.owner_complete_form(owner) else {
            return Ok(None);
        };
        let Some(parts) = component_part_ranges(&owner.head, &owner.head_components) else {
            return Ok(None);
        };
        let Some(byte_ranges) = self.component_byte_ranges(&owner.head, &parts) else {
            return Ok(None);
        };
        let mut name_ranges = Vec::with_capacity(byte_ranges.len());
        let mut previous_end = 0usize;
        for (component, range) in owner.head_components.iter().zip(byte_ranges) {
            if component.role != role
                || !component.has_source_identity()
                || range.start >= range.end
            {
                return Ok(None);
            }
            let Some(gap) = form.get(previous_end..range.start) else {
                return Ok(None);
            };
            let valid_gap = (name_ranges.is_empty() || !gap.is_empty())
                && gap
                    .chars()
                    .all(|c| c.is_whitespace() || matches!(c, ',' | '|'));
            if !valid_gap {
                return Ok(None);
            }
            let Some(visible) = form.get(range.clone()) else {
                return Ok(None);
            };
            let name = if role == OwnerHeadRole::Environment {
                let Some(names) = crate::scan_environment_declaration_names(visible)
                    .map_err(|_| FixedNonOptionLimit::TooManyNames)?
                else {
                    return Ok(None);
                };
                if names.len() != 1 {
                    return Ok(None);
                }
                let Some(single) = names.into_iter().next() else {
                    return Ok(None);
                };
                single
            } else {
                let Some(name) = crate::entry::native_variable_token(visible)
                    .then(|| (visible.to_owned(), 0..visible.len()))
                else {
                    return Ok(None);
                };
                name
            };
            name_ranges.push((name.0, range.start + name.1.start..range.start + name.1.end));
            previous_end = range.end;
        }
        if !form.get(previous_end..).is_some_and(|suffix| {
            suffix
                .chars()
                .all(|c| c.is_whitespace() || matches!(c, ',' | '|'))
        }) {
            return Ok(None);
        }
        let ranges = name_ranges
            .iter()
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        let Some(selections) = self.selection_subranges_from_form(&owner.head, &form, &ranges)
        else {
            return Ok(None);
        };
        Ok(Some(FixedNonOptionRecognition {
            kind,
            evidence: EntryNameEvidence::NativeMarkup,
            occurrences: name_ranges
                .into_iter()
                .zip(selections)
                .map(|((name, _), selection)| (name, selection))
                .collect(),
        }))
    }
}

impl FixedBody {
    /// Check disjoint logical name ranges against final bold runs in one
    /// forward pass. A consumed separator has no glyph style and cannot be
    /// used as a name binding; no complete HEAD copy is made per name.
    pub(super) fn selection_ranges_bold(
        &self,
        selection: &TextSelection,
        form: &str,
        prefix: Range<usize>,
        names: &[Range<usize>],
    ) -> bool {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return false;
        }
        let mut ranges = Vec::with_capacity(names.len() + 1);
        ranges.push(prefix);
        ranges.extend_from_slice(names);
        let mut previous_end = 0;
        for range in &ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return false;
            }
            previous_end = range.end;
        }
        let mut cursor = 0usize;
        let mut target = 0usize;
        let mut covered = ranges[0].start;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                let separator = match &selection.joins[index - 1] {
                    TextJoin::DirectContact => None,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        Some(text)
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return false,
                };
                if let Some(separator) = separator {
                    let Some(end) = cursor.checked_add(separator.len()) else {
                        return false;
                    };
                    if ranges
                        .get(target)
                        .is_some_and(|range| range.start < end && cursor < range.end)
                    {
                        return false;
                    }
                    cursor = end;
                }
            }
            let Some(start_byte) = usize::try_from(part.start_byte).ok() else {
                return false;
            };
            let Some(end_byte) = usize::try_from(part.end_byte).ok() else {
                return false;
            };
            let Some(run) = self.surface.runs.get((part.run.get() - 1) as usize) else {
                return false;
            };
            let Some(visible) = self
                .surface
                .run_text(part.run)
                .and_then(|text| text.get(start_byte..end_byte))
            else {
                return false;
            };
            let Some(end) = cursor.checked_add(visible.len()) else {
                return false;
            };
            while let Some(range) = ranges.get(target)
                && range.start < end
            {
                if range.end <= cursor {
                    return false;
                }
                let begin = range.start.max(cursor);
                let stop = range.end.min(end);
                if begin != covered || !run.label.style.bold {
                    return false;
                }
                covered = stop;
                if covered == range.end {
                    target += 1;
                    if let Some(next) = ranges.get(target) {
                        covered = next.start;
                    }
                } else {
                    break;
                }
            }
            cursor = end;
        }
        target == ranges.len() && cursor == form.len()
    }

    /// Check one PP/RS declaration against its direct native continuation,
    /// without scanning unrelated owners or regions. The paragraph head must
    /// be complete syntax; a textless RS qualifies only with a checked nested
    /// definition head, not merely a table or empty layout scope.
    #[must_use]
    pub fn hanging_declaration_ready(&self, owner: &OwnerMark) -> bool {
        if !self.hanging_structure_ready(owner) {
            return false;
        }
        self.owner_complete_form(owner).is_some_and(|form| {
            crate::entry::is_complete_hanging_option_head_with_provisional(
                &form,
                |prefix, names| self.selection_ranges_bold(&owner.head, &form, prefix, names),
            ) || self.section_is_commands(owner) && crate::manual_call_name_range(&form).is_some()
                || matches!(
                    self.lexical_non_option_declaration(owner, true),
                    Ok(Some(_)) | Err(_)
                )
        })
    }

    /// A native PP/RS pair proves only a reading continuation, while TP/TQ
    /// already owns a real HEAD and BODY. In COMMANDS, a complete
    /// `name(section)` spelling adds independent declaration evidence for
    /// either structure; a merely bold word does not. `man_term.c::pre_TP`
    /// executes the visible HEAD, and `pre_RS` only establishes indentation.
    #[must_use]
    pub fn manual_call_declaration(&self, owner: &OwnerMark) -> Option<FixedNonOptionRecognition> {
        self.manual_call_declaration_with_context(owner, self.nested_under_semantic_parent(owner))
    }

    fn manual_call_declaration_with_context(
        &self,
        owner: &OwnerMark,
        nested: bool,
    ) -> Option<FixedNonOptionRecognition> {
        let mut recognition = self.manual_call_declaration_base(owner)?;
        if nested {
            recognition.kind = EntryKind::Term;
        }
        Some(recognition)
    }

    fn manual_call_declaration_base(&self, owner: &OwnerMark) -> Option<FixedNonOptionRecognition> {
        if owner.role != OwnerRole::Definition
            || !matches!(
                owner.head_role,
                None | Some(OwnerHeadRole::Lexical | OwnerHeadRole::Literal)
            )
            || (owner.hanging_candidate && !self.hanging_structure_ready(owner))
            || !self.section_is_commands(owner)
        {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let range = crate::manual_call_name_range(&form)?;
        let selection = self.selection_subrange(&owner.head, range.clone())?;
        let name = form.get(range)?.to_owned();
        (self.selection_text(&selection).as_deref() == Some(name.as_str())).then_some(
            FixedNonOptionRecognition {
                kind: EntryKind::Command,
                evidence: EntryNameEvidence::Lexical,
                occurrences: vec![(name, selection)],
            },
        )
    }

    /// A structurally true definition with no specialized macro role may
    /// still have a complete, multiword term. The entire checked label is its
    /// name; no first-word option/command guess is made from display style.
    #[doc(hidden)]
    #[must_use]
    pub fn plain_term_name(&self, owner: &OwnerMark) -> Option<(String, TextSelection)> {
        if owner.role != OwnerRole::Definition
            || !matches!(owner.head_role, None | Some(OwnerHeadRole::Lexical))
            || owner.hanging_candidate
        {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let styled_marker = self.styled_presentation_label(owner, &form);
        let range = crate::complete_term_label_range(&form).or_else(|| {
            styled_marker.then(|| {
                let start = form.len() - form.trim_start().len();
                start..start + form.trim().len()
            })
        })?;
        // A failed italic/roman option candidate is not converted into a
        // named Term merely because TP provided a physical head. The shared
        // option grammar must prove that spelling independently.
        if crate::option_prefix(form.get(range.clone())?).is_some() {
            return None;
        }
        if owner.head_role == Some(OwnerHeadRole::Lexical)
            && !owner.lexical_term_witness
            && !styled_marker
            && (owner.head_components.is_empty() || !self.complete_head_bold(&owner.head))
        {
            return None;
        }
        let selection = self.selection_subrange(&owner.head, range.clone())?;
        let name = form.get(range)?.to_owned();
        (self.selection_text(&selection).as_deref() == Some(name.as_str()))
            .then_some((name, selection))
    }

    fn complete_head_bold(&self, head: &TextSelection) -> bool {
        head.parts.iter().all(|part| {
            let Some(run) = self.surface.runs.get((part.run.get() - 1) as usize) else {
                return false;
            };
            let Some(text) = self.surface.run_text(part.run).and_then(|text| {
                let start = usize::try_from(part.start_byte).ok()?;
                let end = usize::try_from(part.end_byte).ok()?;
                text.get(start..end)
            }) else {
                return false;
            };
            run.label.style.bold || text.chars().all(char::is_whitespace)
        })
    }

    fn styled_presentation_label(&self, owner: &OwnerMark, form: &str) -> bool {
        // man_macro.c::blk_imp gives IP its own displayed head, while a TP
        // child macro is separately retained as a native component. An
        // explicitly bold IP punctuation key remains addressable; a plain
        // marker or a styled TP list bullet does not gain that evidence.
        let mut glyphs = form.trim().chars();
        owner.role == OwnerRole::Definition
            && owner.head_role == Some(OwnerHeadRole::Lexical)
            && !owner.hanging_candidate
            && !owner.lexical_term_witness
            && owner.head_components.is_empty()
            && glyphs.next().is_some_and(|glyph| glyph.is_ascii())
            && glyphs.next().is_none()
            && crate::is_presentation_term(form)
            && self.complete_head_bold(&owner.head)
    }

    /// A formatter marker can occupy a man TP head without becoming a name.
    /// mdoc bullet/enum owners are already excluded by `OwnerRole::Other`.
    #[doc(hidden)]
    #[must_use]
    pub fn presentation_only_head(&self, owner: &OwnerMark) -> bool {
        owner.role == OwnerRole::Definition
            && !matches!(owner.head_role, Some(OwnerHeadRole::Option))
            && self.owner_complete_form(owner).is_some_and(|form| {
                (crate::is_presentation_term(&form)
                    || owner.head_role == Some(OwnerHeadRole::Literal) && form.trim() == "--")
                    && self.literal_dash_recognition(owner, &form).is_none()
                    && !self.styled_presentation_label(owner, &form)
            })
    }

    fn hanging_structure_ready(&self, owner: &OwnerMark) -> bool {
        let Some(region) = owner
            .hanging_continuation
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
        else {
            return false;
        };
        let nested_head_ready = owner
            .hanging_nested_head
            .and_then(|key| self.regions.get((key.get() - 1) as usize))
            .is_some_and(|head| {
                head.kind == RegionKind::OwnerHead
                    && head.parent == Some(region.key)
                    && head.section == owner.section
                    && head.owner.is_some_and(|nested| {
                        nested != owner.key
                            && self
                                .owners
                                .get((nested.get() - 1) as usize)
                                .is_some_and(|child| {
                                    child.section == owner.section
                                        && child.parent == owner.parent
                                        && child.role == OwnerRole::Definition
                                        && child.head == head.selection
                                })
                    })
            });
        owner.hanging_candidate
            && owner.role == OwnerRole::Definition
            && owner.head_role == Some(OwnerHeadRole::Lexical)
            && region.kind == RegionKind::HangingContinuation
            && region.continuation_of == Some(owner.key)
            && region.owner.is_none()
            && region.section == owner.section
            && (owner.hanging_nested_head.is_none() || nested_head_ready)
            && (!region.selection.parts.is_empty() || nested_head_ready)
    }

    /// Select names from one complete native lexical head. Source-neutral
    /// spellings and native argument style pass the same checked intervals.
    pub(super) fn lexical_literal_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return None;
        }
        let form = self.owner_complete_form(owner)?;
        let scan = self.lexical_declaration_recognition(owner, &form)?;
        let crate::RecognitionParts {
            ranges: segments,
            names: literal,
            over_limit,
        } = scan.into_parts();
        if over_limit {
            return None;
        }
        // With no candidate there is no name-to-glyph mapping to perform.
        // checked_lexical_names also accepts this empty, non-rejected case.
        if literal.is_empty() {
            return Some(Vec::new());
        }
        let ranges = literal
            .iter()
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        let selections = self.selection_subranges_from_form(&owner.head, &form, &ranges)?;
        let mut names = Vec::with_capacity(literal.len());
        for ((name, range), selection) in literal.into_iter().zip(selections) {
            if form.get(range.clone()) != Some(name.as_str()) {
                return None;
            }
            names.push((name, selection, range));
        }
        let names = self.checked_lexical_names(owner, names, &segments)?;
        // The parser-alive prefix is a candidate for the first declaration,
        // not permission to ignore the rest of the native HEAD.  Keep it
        // tied to the same final glyphs when it was recorded.
        if !names.is_empty()
            && let Some(prefix) = owner.head_role_prefix.as_deref()
            && names.first().map(|(name, _, _)| name.as_str()) != Some(prefix)
        {
            return None;
        }
        Some(names)
    }

    /// One checked result for producer, validator, and query positions.
    /// Native operand components describe where glyphs came from; a prefix
    /// inside one component is not an independent name alongside the complete
    /// displayed HEAD (`man_term.c::pre_alternate` joins its operands). A
    /// nonempty result contains exact names; an empty result proves a checked
    /// non-option head; `None` denotes incomplete or contradictory evidence.
    #[must_use]
    pub fn lexical_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        self.lexical_literal_names(owner)
    }

    /// A bold native operand begins another declaration only at its own
    /// final visible glyphs. In particular, punctuation inside the preceding
    /// italic parameter cannot manufacture such an operand. This is a
    /// bounded annotation of the complete displayed HEAD, not a second
    /// source-spelling parser or a requirement for authored coordinates.
    #[expect(
        clippy::too_many_lines,
        reason = "collect one owner's native component and final-style evidence in display order"
    )]
    pub(super) fn lexical_declaration_recognition(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<crate::entry::RecognitionResult> {
        let component_ranges = component_part_ranges(&owner.head, &owner.head_components)
            .and_then(|parts| self.component_byte_ranges(&owner.head, &parts))
            .map(|ranges| owner.head_components.iter().zip(ranges).collect::<Vec<_>>())
            .unwrap_or_default();
        // Only native macro components are independent operands. A lone `.IP`
        // label is one text operand in man_term.c::pre_IP; `\fB` within it
        // may change the final run style without ending a parameter.
        let operands = component_ranges
            .iter()
            .filter_map(|(component, range)| {
                (component.role == OwnerHeadRole::Lexical && range.start < range.end)
                    .then_some(range.clone())
            })
            .collect::<Vec<_>>();
        // Build final-bold glyph intervals once in logical HEAD coordinates.
        // Calling selection_subrange for each numeric-looking native child
        // would repeatedly rebuild and scan the whole HEAD (quadratic for a
        // long alternating macro). Joins occupy logical bytes but no glyphs,
        // so a name crossing one cannot accidentally inherit a font style.
        if owner.head.joins.len() != owner.head.parts.len().saturating_sub(1) {
            return None;
        }
        let mut bold_spans: Vec<Range<usize>> = Vec::new();
        let mut glyph_cursor = 0usize;
        for (index, part) in owner.head.parts.iter().enumerate() {
            if index != 0 {
                glyph_cursor = glyph_cursor.checked_add(match &owner.head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            let start = glyph_cursor;
            let run = self.surface.runs.get((part.run.get() - 1) as usize)?;
            let visible = self.surface.run_text(part.run)?.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            glyph_cursor = glyph_cursor.checked_add(visible.len())?;
            if run.label.style.bold {
                if let Some(last) = bold_spans.last_mut()
                    && last.end == start
                {
                    last.end = glyph_cursor;
                } else {
                    bold_spans.push(start..glyph_cursor);
                }
            }
        }
        if glyph_cursor != form.len() {
            return None;
        }
        // A signed number in an existing argument is not an option. The
        // exception for a short numeric flag requires this native lexical
        // component's first surviving glyphs, their authored source identity,
        // and the final bold display of both glyphs. pre_alternate() supplies
        // the operand boundary; term_word() supplies the executed glyph/style.
        let mut bold_cursor = 0usize;
        let numeric_name_starts = component_ranges
            .iter()
            .filter_map(|(component, range)| {
                if component.role != OwnerHeadRole::Lexical || !component.has_source_identity() {
                    return None;
                }
                let visible = form.get(range.clone())?;
                let start = range.start + visible.len() - visible.trim_start().len();
                let end = start.checked_add(2)?;
                let name = form.get(start..end)?;
                let bytes = name.as_bytes();
                if end > range.end
                    || bytes.len() != 2
                    || bytes[0] != b'-'
                    || !bytes[1].is_ascii_digit()
                {
                    return None;
                }
                while bold_spans
                    .get(bold_cursor)
                    .is_some_and(|span| span.end <= start)
                {
                    bold_cursor += 1;
                }
                bold_spans
                    .get(bold_cursor)
                    .is_some_and(|span| span.start <= start && end <= span.end)
                    .then_some(start)
            })
            .collect::<Vec<_>>();
        // Component intervals and display parts are both in visible order.
        // Walk their starts once; a tree lookup for every font fragment would
        // make a long alternating HEAD needlessly superlinear.
        let mut operand_boundary = 0;
        let mut plain_italic_ranges: Vec<Range<usize>> = Vec::new();
        let mut bold_underline_starts = Vec::new();
        let mut offset = 0usize;
        let mut previous_style = None;
        let mut underlined_group_started = false;
        for (index, part) in owner.head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &owner.head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            let start = offset;
            let run = self.surface.runs.get((part.run.get() - 1) as usize)?;
            let visible = self.surface.run_text(part.run)?.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            offset = offset.checked_add(visible.len())?;
            let style = (run.label.style.bold, run.label.style.underline);
            while operands
                .get(operand_boundary)
                .is_some_and(|operand| operand.start < start)
            {
                operand_boundary += 1;
            }
            let starts_operand = operands
                .get(operand_boundary)
                .is_some_and(|operand| operand.start == start);
            let same_style_group = index != 0
                && owner.head.joins[index - 1] == TextJoin::DirectContact
                && !starts_operand
                && previous_style == Some(style);
            if !same_style_group {
                underlined_group_started = false;
            }
            if run.label.style.underline {
                if underlined_group_started && same_style_group && !run.label.style.bold {
                    plain_italic_ranges.last_mut()?.end = offset;
                } else if !underlined_group_started
                    && let Some(first) = visible.find(|character: char| !character.is_whitespace())
                {
                    let visible_start = start.checked_add(first)?;
                    if run.label.style.bold {
                        bold_underline_starts.push(visible_start);
                    } else {
                        plain_italic_ranges.push(visible_start..offset);
                    }
                    underlined_group_started = true;
                }
            }
            previous_style = Some(style);
        }
        if offset != form.len() {
            return None;
        }
        crate::recognize_option_declarations(
            crate::DeclarationView {
                visible: form,
                native_operands: &operands,
                plain_italic_ranges: &plain_italic_ranges,
                bold_underline_starts: &bold_underline_starts,
                numeric_name_starts: &numeric_name_starts,
            },
            if owner.head_components.is_empty() {
                crate::DeclarationContext::SingleTextOperand
            } else {
                crate::DeclarationContext::NativeComponents
            },
        )
    }

    fn checked_lexical_names(
        &self,
        owner: &OwnerMark,
        candidates: Vec<(String, TextSelection, std::ops::Range<usize>)>,
        segments: &[Range<usize>],
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        let had_candidates = !candidates.is_empty();
        let mut names = Vec::new();
        let mut blocked_segment = None;
        let mut segment_cursor = 0;
        let mut style_rejected = false;
        for (name, selection, range) in candidates {
            while segments
                .get(segment_cursor)
                .is_some_and(|segment| segment.end < range.start)
            {
                segment_cursor += 1;
            }
            let segment = segments.get(segment_cursor)?;
            if segment.start > range.start || range.end > segment.end {
                return None;
            }
            if blocked_segment == Some(segment_cursor) {
                continue;
            }
            // Underline is final native display evidence, not a recovered
            // italic opcode. Conservatively treat an underlined nonbold name
            // as a parameter within this segment only; a later independently
            // delimited declaration remains eligible.
            if selection.parts.iter().any(|part| {
                self.surface
                    .runs
                    .get((part.run.get() - 1) as usize)
                    .is_some_and(|run| run.label.style.underline && !run.label.style.bold)
            }) {
                style_rejected = true;
                blocked_segment = Some(segment_cursor);
                continue;
            }
            // A plain IP label has no independent lexical component or TP/TQ
            // term witness. Bold `-a` glued to roman `foo` is only a styled
            // label, not a proved combined option. In contrast, pre_B() is
            // an explicit macro instance and term_word() may switch to roman
            // inside its one complete spelling without ending that name.
            if !owner.lexical_term_witness && owner.head_components.is_empty() {
                let mut prior_bold = None;
                let mut mixed_bold = false;
                for part in &selection.parts {
                    let current_bold = self
                        .surface
                        .runs
                        .get((part.run.get() - 1) as usize)?
                        .label
                        .style
                        .bold;
                    if prior_bold
                        .replace(current_bold)
                        .is_some_and(|before| before != current_bold)
                    {
                        mixed_bold = true;
                    }
                }
                if mixed_bold {
                    style_rejected = true;
                    blocked_segment = Some(segment_cursor);
                    continue;
                }
            }
            // A B operand can switch to roman in the middle of one visible
            // spelling: pre_B() chooses only its initial font and term_word()
            // executes \fR inline. The shared scan already found the name
            // interval; a boldness change is not a second parameter grammar.
            names.push((name, selection, range));
        }
        if names.len() > 64 || names.windows(2).any(|pair| pair[0].2.end > pair[1].2.start) {
            return None;
        }
        // An empty result is meaningful only when final italic styling
        // rejected a candidate: the producer must not reclassify that same
        // visible spelling through the generic identity fallback. A head
        // with no option candidate remains eligible as an ordinary Term.
        (!names.is_empty() || style_rejected || !had_candidates).then_some(names)
    }

    /// Bind each source-identified `Fl` macro to its own final glyphs without
    /// requiring those names to cover the entire `HEAD`.
    /// `mdoc_macro.c::blk_full()` keeps `Ar` operands in the same `It` `HEAD`,
    /// and `mdoc_term.c::termp_fl_pre()` prints every distinct `Fl` invocation.
    /// The complete `HEAD` remains one form; these components prove names, not
    /// separate forms or alias relationships.
    /// A sixty-fifth proved invocation invalidates the whole semantic group,
    /// even if its spelling repeats an earlier one.
    #[must_use]
    pub fn option_component_names(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection, std::ops::Range<usize>)>> {
        if owner.head_role != Some(OwnerHeadRole::Option)
            || owner.head_components.len() < 2
            || self.owner_complete_form(owner).is_none()
        {
            return None;
        }
        let ranges = component_part_ranges(&owner.head, &owner.head_components)?;
        let byte_ranges = self.component_byte_ranges(&owner.head, &ranges)?;
        let mut names = Vec::with_capacity(owner.head_components.len().min(64));
        for (component, range) in owner.head_components.iter().zip(byte_ranges) {
            if component.selection.parts.is_empty() || !component.has_source_identity() {
                return None;
            }
            // A mixed It HEAD can contain Cm/Ic alongside distinct Fl
            // invocations. Only Fl is option-name evidence; a literal that
            // happens to spell like an option must not veto sibling Fl names
            // or itself become one.
            if component.role != OwnerHeadRole::Option {
                continue;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) {
                // Each Fl is an independent native invocation. In particular,
                // mdoc_term.c::termp_fl_pre() emits a visible dash even when
                // the operand has no glyphs. That instance proves no name,
                // but must not erase names proved by sibling Fl instances.
                continue;
            }
            if names.len() == 64 {
                return None;
            }
            names.push((text, component.selection.clone(), range));
        }
        (!names.is_empty()).then_some(names)
    }

    /// Whether more than 64 independently visible `Fl` invocations prove
    /// option names in one head. The producer checks this before any lexical
    /// or first-component fallback; the IR validator applies the same gate to
    /// externally supplied entry facts. Neither may publish a truncated group.
    #[must_use]
    pub fn option_component_over_limit(&self, owner: &OwnerMark) -> bool {
        if owner.head_role != Some(OwnerHeadRole::Option) {
            return false;
        }
        let mut proved = 0;
        for component in &owner.head_components {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                continue;
            }
            if self
                .selection_text(&component.selection)
                .is_some_and(|text| crate::native_option_token(&text))
            {
                proved += 1;
                if proved > 64 {
                    return true;
                }
            }
        }
        false
    }

    fn component_byte_ranges(
        &self,
        head: &TextSelection,
        part_ranges: &[std::ops::Range<usize>],
    ) -> Option<Vec<std::ops::Range<usize>>> {
        let mut output = Vec::with_capacity(part_ranges.len());
        let mut offset = 0usize;
        let mut component = 0usize;
        let mut start = 0usize;
        for (index, part) in head.parts.iter().enumerate() {
            if index != 0 {
                offset = offset.checked_add(match &head.joins[index - 1] {
                    TextJoin::DirectContact => 0,
                    TextJoin::AuthoredSeparator(text) | TextJoin::GeneratedSeparator(text) => {
                        text.len()
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                })?;
            }
            if part_ranges
                .get(component)
                .is_some_and(|range| range.start == index)
            {
                start = offset;
            }
            let run = self.surface.run_text(part.run)?;
            let text = run.get(
                usize::try_from(part.start_byte).ok()?..usize::try_from(part.end_byte).ok()?,
            )?;
            offset = offset.checked_add(text.len())?;
            if part_ranges
                .get(component)
                .is_some_and(|range| range.end == index + 1)
            {
                output.push(start..offset);
                component += 1;
            }
        }
        (component == part_ranges.len()).then_some(output)
    }

    /// Return multiple exact option declarations only when distinct native
    /// `Fl` macro instances cover every non-separator glyph in one complete
    /// definition HEAD. Typography or punctuation alone never creates names.
    #[must_use]
    pub fn option_component_forms(
        &self,
        owner: &OwnerMark,
    ) -> Option<Vec<(String, TextSelection)>> {
        if owner.role != OwnerRole::Definition || owner.head_components.len() < 2 {
            return None;
        }
        let mut forms = Vec::with_capacity(owner.head_components.len().min(64));
        let mut seen = BTreeSet::new();
        for component in &owner.head_components {
            if component.role != OwnerHeadRole::Option
                || component.selection.parts.is_empty()
                || !component.has_source_identity()
            {
                return None;
            }
            let text = self.selection_text(&component.selection)?;
            if !crate::native_option_token(&text) || !seen.insert(text.clone()) {
                return None;
            }
            if forms.len() == 64 {
                return None;
            }
            forms.push((text, component.selection.clone()));
        }
        let mut selected = owner
            .head_components
            .iter()
            .flat_map(|component| component.selection.parts.iter())
            .peekable();
        for part in &owner.head.parts {
            if selected.peek().is_some_and(|component| *component == part) {
                selected.next();
                continue;
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            if !run
                .get(start..end)?
                .bytes()
                .all(|byte| byte.is_ascii_whitespace() || matches!(byte, b',' | b'|' | b'/'))
            {
                return None;
            }
        }
        selected.next().is_none().then_some(forms)
    }

    /// Keys whose optional entry facts fail the same read-time proof used by
    /// consumers. Producers can retract only these facts before finalizing a
    /// document, without discarding the native body or valid sibling entries.
    #[must_use]
    pub fn invalid_entry_keys(&self) -> Vec<NonZeroU32> {
        let pass = self.entry_pass();
        self.owners
            .iter()
            .filter(|owner| owner.entry.is_some() && pass.validated_entry(owner).is_none())
            .map(|owner| owner.key)
            .collect()
    }

    /// Native output remains readable when a link target fails semantic
    /// validation; only these occurrences must lose activation.
    #[must_use]
    pub fn invalid_link_target_keys(&self) -> Vec<NonZeroU32> {
        self.links
            .iter()
            .filter(|link| {
                link.target
                    .as_ref()
                    .is_some_and(|target| validation::validate_link_target(target).is_err())
            })
            .map(|link| link.key)
            .collect()
    }

    /// Borrow only the current conservative Fixed facts whose form, name and
    /// lexical binding close against this owner's surviving native head.
    /// Recheck at read time: an in-memory `Document` can be changed after its
    /// deserialization guard ran.
    #[allow(clippy::too_many_lines)] // One read-time closure of all entry fact variants.
    pub(crate) fn validated_entry<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<&'a EntryFacts<TextSelection>> {
        self.validated_entry_with_context(owner, self.nested_under_semantic_parent(owner))
    }

    #[allow(clippy::too_many_lines)] // One read-time closure of all entry fact variants.
    fn validated_entry_with_context<'a>(
        &self,
        owner: &'a OwnerMark,
        nested: bool,
    ) -> Option<&'a EntryFacts<TextSelection>> {
        let entry = owner.entry.as_ref()?;
        // A bullet/ordinal owner is a readable list item, not a definition.
        // The native mdoc It list type, not its generated glyph, decides this.
        if owner.role != OwnerRole::Definition {
            return None;
        }
        if self.option_component_over_limit(owner) {
            return None;
        }
        if owner.hanging_candidate {
            if !self.hanging_declaration_ready(owner) {
                return None;
            }
        } else if owner.hanging_continuation.is_some() || owner.hanging_nested_head.is_some() {
            return None;
        }
        if entry.forms.len() > 1 {
            let forms = self.option_component_forms(owner)?;
            let valid = entry.id == owner.id
                && entry.kind
                    == EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    }
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && entry.forms.len() == forms.len()
                && entry.names.len() == forms.len()
                && entry.name_bindings.len() == forms.len()
                && forms.iter().enumerate().all(|(index, (name, selection))| {
                    entry.names[index] == *name
                        && entry.forms[index] == *selection
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences.as_slice()
                            == std::slice::from_ref(selection)
                });
            return valid.then_some(entry);
        }
        if let Some(recognition) = self.manual_call_declaration_with_context(owner, nested) {
            return Self::validated_non_option_names(owner, entry, recognition).then_some(entry);
        }
        if let Some(recognition) = self
            .scan_non_option_declaration_with_context(owner, nested)
            .ok()
            .flatten()
        {
            return Self::validated_non_option_names(owner, entry, recognition).then_some(entry);
        }
        if (self.unbound_environment_head(owner) || self.unbound_variable_head(owner))
            && !matches!(
                entry.kind,
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option
                }
            )
        {
            return None;
        }
        if self.presentation_only_head(owner) {
            return None;
        }
        if matches!(
            owner.head_role,
            Some(
                OwnerHeadRole::Environment
                    | OwnerHeadRole::Variable
                    | OwnerHeadRole::DefinedVariable
            )
        ) {
            return None;
        }
        let Some(form) = self.owner_complete_form(owner) else {
            // mdoc_macro.c::blk_full may keep a long Xo HEAD whose later
            // output joins are unknown. Its first Ic/Cm component can still
            // prove a complete command word; no other entry kind may borrow
            // this partial form or infer the rest of the HEAD.
            return self
                .validated_partial_literal_name(owner, entry, nested)
                .then_some(entry);
        };
        let [only_form] = entry.forms.as_slice() else {
            return None;
        };
        if entry.kind == EntryKind::Term
            && let Some((name, selection)) = self.plain_term_name(owner)
        {
            let valid = entry.id == owner.id
                && entry.kind == EntryKind::Term
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && only_form == &owner.head
                && entry.names.as_slice() == std::slice::from_ref(&name)
                && entry.name_bindings.as_slice()
                    == std::slice::from_ref(&crate::EntryNameBinding {
                        name: 0,
                        occurrences: vec![selection],
                        evidence: EntryNameEvidence::Lexical,
                    });
            return valid.then_some(entry);
        }
        if entry.kind == EntryKind::Term
            && (owner.head_role.is_none()
                || owner.head_role == Some(OwnerHeadRole::Lexical)
                    && self
                        .lexical_names(owner)
                        .is_some_and(|names| names.is_empty()))
        {
            let valid = entry.id == owner.id
                && entry.case == NameCase::Sensitive
                && entry.alias_groups.is_empty()
                && entry.alias_of.is_none()
                && entry.value_domain.is_none()
                && only_form == &owner.head
                && entry.names.is_empty()
                && entry.name_bindings.is_empty();
            return valid.then_some(entry);
        }
        if owner.head_role == Some(OwnerHeadRole::Option)
            && owner.head_components.len() > 1
            && self.option_component_names(owner).is_some()
        {
            return self
                .validated_component_names(owner, entry, only_form)
                .then_some(entry);
        }
        if entry.names.len() > 1 {
            return (self.validated_lexical_names(owner, entry, only_form)
                || self.validated_component_names(owner, entry, only_form))
            .then_some(entry);
        }
        let [only_name] = entry.names.as_slice() else {
            return None;
        };
        let [binding] = entry.name_bindings.as_slice() else {
            return None;
        };
        let binding_matches = match (owner.head_role, entry.kind, binding.evidence) {
            (_, EntryKind::Term, EntryNameEvidence::Lexical) => {
                only_name == &form
                    && binding.occurrences.as_slice() == std::slice::from_ref(&owner.head)
                    && !matches!(owner.head_role, None | Some(OwnerHeadRole::Lexical))
            }
            (
                Some(OwnerHeadRole::Lexical),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::Lexical,
            ) => self.validated_lexical_names(owner, entry, only_form),
            (
                Some(OwnerHeadRole::Option),
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                EntryNameEvidence::NativeMarkup,
            )
            | (
                Some(OwnerHeadRole::Environment),
                EntryKind::EnvironmentVariable,
                EntryNameEvidence::NativeMarkup,
            )
            | (
                Some(OwnerHeadRole::Literal),
                EntryKind::Command | EntryKind::ConfigurationKey | EntryKind::Term,
                EntryNameEvidence::NativeMarkup,
            ) => self.native_markup_name_matches(
                owner,
                entry.kind,
                &form,
                only_name,
                &binding.occurrences,
                nested,
            ),
            _ => false,
        };
        (entry.id == owner.id
            && binding_matches
            && entry.case == NameCase::Sensitive
            && only_form == &owner.head
            && binding.name == 0
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none())
        .then_some(entry)
    }

    fn validated_non_option_names(
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        recognition: FixedNonOptionRecognition,
    ) -> bool {
        let grouped = group_name_occurrences(recognition.occurrences);
        if entry.id != owner.id
            || entry.kind != recognition.kind
            || entry.case != NameCase::Sensitive
            || !entry.alias_groups.is_empty()
            || entry.alias_of.is_some()
            || entry.value_domain.is_some()
            || entry.forms.as_slice() != std::slice::from_ref(&owner.head)
            || entry.names.len() != grouped.len()
            || entry.name_bindings.len() != grouped.len()
            || !entry
                .names
                .iter()
                .zip(&grouped)
                .all(|(actual, (expected, _))| actual == expected)
        {
            return false;
        }
        // Binding order is independent of name order. Every binding must
        // close against the exact surviving occurrence set for its index;
        // reusing a prefix, missing a duplicate, or claiming another name is
        // not a valid non-option declaration.
        let mut seen = vec![false; grouped.len()];
        for binding in &entry.name_bindings {
            let Some((_, occurrences)) = grouped.get(binding.name) else {
                return false;
            };
            if std::mem::replace(&mut seen[binding.name], true)
                || binding.evidence != recognition.evidence
                || binding.occurrences != *occurrences
            {
                return false;
            }
        }
        seen.into_iter().all(|found| found)
    }

    fn validated_partial_literal_name(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        nested: bool,
    ) -> bool {
        let Some(recognition) = self.partial_literal_declaration_with_context(owner, nested) else {
            return false;
        };
        let [(name, component)] = recognition.occurrences.as_slice() else {
            return false;
        };
        let ([only_form], [only_name], [binding]) = (
            entry.forms.as_slice(),
            entry.names.as_slice(),
            entry.name_bindings.as_slice(),
        ) else {
            return false;
        };
        entry.id == owner.id
            && entry.kind == recognition.kind
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == component
            && only_name == name
            && binding.name == 0
            && binding.evidence == recognition.evidence
            && binding.occurrences.as_slice() == std::slice::from_ref(component)
    }

    fn native_markup_name_matches(
        &self,
        owner: &OwnerMark,
        kind: EntryKind,
        form: &str,
        name: &str,
        occurrences: &[TextSelection],
        nested: bool,
    ) -> bool {
        self.native_head_identity_with_context(owner, form, nested)
            .is_some_and(|(proved_kind, evidence, proved_name, selection)| {
                proved_kind == kind
                    && evidence == EntryNameEvidence::NativeMarkup
                    && proved_name == name
                    && occurrences == std::slice::from_ref(&selection)
            })
    }

    /// The same complete, surviving native prefix proof used by the Fixed
    /// producer and by read-time fact validation. An authored macro role is
    /// only a candidate: its name must still bind to final visible glyphs.
    #[doc(hidden)]
    #[must_use]
    pub fn native_head_identity(
        &self,
        owner: &OwnerMark,
        form: &str,
    ) -> Option<(EntryKind, EntryNameEvidence, String, TextSelection)> {
        self.native_head_identity_with_context(
            owner,
            form,
            self.nested_under_semantic_parent(owner),
        )
    }

    fn native_head_identity_with_context(
        &self,
        owner: &OwnerMark,
        form: &str,
        nested: bool,
    ) -> Option<(EntryKind, EntryNameEvidence, String, TextSelection)> {
        let leading = form.trim_start();
        let start = form.len() - leading.len();
        if owner.head_role == Some(OwnerHeadRole::Literal) {
            let prefix = self.literal_prefix_proof(owner)?;
            let end = start.checked_add(prefix.name.len())?;
            // This conservative fallback must obey the same base syntax as
            // the complete Literal scan. Parent context may weaken the
            // category only after the prefix has passed that syntax; it
            // cannot turn an unbound key/value spelling into a Term name.
            let kind = if nested {
                EntryKind::Term
            } else {
                prefix.name_kind
            };
            if form.get(start..end) != Some(prefix.name.as_str())
                || !form.get(end..).is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                })
                || self.selection_subrange(&owner.head, start..end).as_ref()
                    != Some(prefix.selection)
                || prefix.base_kind == EntryKind::ConfigurationKey
                    && !form
                        .get(end..)
                        .is_some_and(|suffix| suffix.trim().is_empty())
            {
                return None;
            }
            return Some((
                kind,
                EntryNameEvidence::NativeMarkup,
                prefix.name,
                prefix.selection.clone(),
            ));
        }
        let role_prefix = owner.head_role_prefix.as_deref()?;
        if !leading.starts_with(role_prefix) {
            return None;
        }
        let (kind, name) = match owner.head_role? {
            OwnerHeadRole::Option => crate::native_option_token(role_prefix).then(|| {
                (
                    EntryKind::Parameter {
                        parameter_kind: ParameterKind::Option,
                    },
                    role_prefix.to_owned(),
                )
            })?,
            OwnerHeadRole::Environment => (
                EntryKind::EnvironmentVariable,
                crate::environment_variable_alias(role_prefix)?,
            ),
            OwnerHeadRole::Lexical
            | OwnerHeadRole::Literal
            | OwnerHeadRole::Argument
            | OwnerHeadRole::Variable
            | OwnerHeadRole::DefinedVariable => return None,
        };
        let end = start.checked_add(name.len())?;
        if owner.head_role == Some(OwnerHeadRole::Lexical)
            && !matches!(form.get(end..)?.chars().next(), None | Some('='))
            && !form.get(end..)?.starts_with(char::is_whitespace)
        {
            return None;
        }
        let occurrence = self.selection_subrange(&owner.head, start..end)?;
        (self.selection_text(&occurrence).as_deref() == Some(name.as_str())).then_some((
            kind,
            EntryNameEvidence::NativeMarkup,
            name,
            occurrence,
        ))
    }

    /// A first native Ic/Cm component may prove one complete command token
    /// even when a later Xo HEAD join is unknown. The component must be the
    /// exact visible prefix and must end at a proved word boundary; layout
    /// adjacency alone never supplies that boundary.
    #[must_use]
    pub fn literal_command_component<'a>(
        &self,
        owner: &'a OwnerMark,
    ) -> Option<(String, &'a TextSelection)> {
        if owner.role != OwnerRole::Definition || owner.head_role != Some(OwnerHeadRole::Literal) {
            return None;
        }
        let component = owner.head_components.first()?;
        if component.role != OwnerHeadRole::Literal || !component.has_source_identity() {
            return None;
        }
        let name = self.selection_text(&component.selection)?;
        if !crate::native_command_token(&name) || component.selection.parts.is_empty() {
            return None;
        }
        let count = component.selection.parts.len();
        if count > owner.head.parts.len()
            || component.selection.joins.as_slice() != owner.head.joins.get(..count - 1)?
            || component.selection.parts.as_slice() != owner.head.parts.get(..count)?
        {
            return None;
        }
        let boundary = if count == owner.head.parts.len() {
            true
        } else {
            match owner.head.joins.get(count - 1)? {
                TextJoin::AuthoredSeparator(separator)
                | TextJoin::GeneratedSeparator(separator) => {
                    separator.starts_with(char::is_whitespace)
                }
                TextJoin::DirectContact => {
                    let next = &owner.head.parts[count];
                    let text = self.surface.run_text(next.run)?;
                    text.get(usize::try_from(next.start_byte).ok()?..)?
                        .starts_with(char::is_whitespace)
                }
                TextJoin::HardBoundary | TextJoin::Unknown => false,
            }
        };
        boundary.then_some((name, &component.selection))
    }

    fn literal_prefix_proof<'a>(&self, owner: &'a OwnerMark) -> Option<LiteralPrefixProof<'a>> {
        let (name, selection) = self.literal_command_component(owner)?;
        let base_kind = self.base_literal_entry_kind(owner);
        let full = 0..name.len();
        let name_kind = match Self::literal_name_range(&name, base_kind) {
            Some(range) if range == full => base_kind,
            // A complete component with an attached value has a shorter
            // configuration name. Partial HEADs cannot bind its whole text.
            None if base_kind == EntryKind::ConfigurationKey
                && Self::literal_name_range(&name, EntryKind::Term) == Some(full) =>
            {
                EntryKind::Term
            }
            Some(_) | None => return None,
        };
        Some(LiteralPrefixProof {
            base_kind,
            name_kind,
            name,
            selection,
        })
    }

    /// Close every lexical alias against the same original HEAD and one
    /// exact, surviving display sub-selection. The syntax cannot stand in for
    /// the native role or for a missing glyph range.
    fn validated_lexical_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        if owner.head_role != Some(OwnerHeadRole::Lexical) {
            return false;
        }
        let Some(found) = self.lexical_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            found
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::Lexical
                        && entry.name_bindings[index].occurrences == *occurrences
                })
    }

    fn validated_component_names(
        &self,
        owner: &OwnerMark,
        entry: &EntryFacts<TextSelection>,
        only_form: &TextSelection,
    ) -> bool {
        let Some(names) = self.option_component_names(owner) else {
            return false;
        };
        let grouped = group_name_occurrences(
            names
                .into_iter()
                .map(|(name, selection, _)| (name, selection)),
        );
        entry.id == owner.id
            && entry.kind
                == EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                }
            && entry.case == NameCase::Sensitive
            && entry.alias_groups.is_empty()
            && entry.alias_of.is_none()
            && entry.value_domain.is_none()
            && only_form == &owner.head
            && entry.names.len() == grouped.len()
            && entry.name_bindings.len() == grouped.len()
            && grouped
                .iter()
                .enumerate()
                .all(|(index, (name, occurrences))| {
                    entry.names[index] == *name
                        && entry.name_bindings[index].name == index
                        && entry.name_bindings[index].evidence == EntryNameEvidence::NativeMarkup
                        && entry.name_bindings[index].occurrences == *occurrences
                })
    }

    /// Project a checked final-display selection into logical text without
    /// copying the surface into a second stored body.
    #[must_use]
    pub fn selection_text(&self, selection: &TextSelection) -> Option<String> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut text = String::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => text.push_str(separator),
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let run = self.surface.run_text(part.run)?;
            let start = usize::try_from(part.start_byte).ok()?;
            let end = usize::try_from(part.end_byte).ok()?;
            text.push_str(run.get(start..end)?);
        }
        Some(text)
    }

    /// Map a logical UTF-8 range back to surviving display slices. A range
    /// touching a consumed authored or generated separator has no final
    /// glyph to bind and is rejected; no byte or cell coordinate is inferred
    /// from layout.
    #[must_use]
    pub fn selection_subrange(
        &self,
        selection: &TextSelection,
        range: Range<usize>,
    ) -> Option<TextSelection> {
        let logical = self.selection_text(selection)?;
        if range.start >= range.end || logical.get(range.clone()).is_none() {
            return None;
        }
        let mut cursor = 0usize;
        let mut parts = Vec::new();
        let mut joins = Vec::new();
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if range.start < end && cursor < range.end {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let length = usize::try_from(part.end_byte.checked_sub(part.start_byte)?).ok()?;
            let end = cursor.checked_add(length)?;
            let start_in_part = range.start.max(cursor);
            let end_in_part = range.end.min(end);
            if start_in_part < end_in_part {
                let start_byte = part
                    .start_byte
                    .checked_add(u64::try_from(start_in_part - cursor).ok()?)?;
                let end_byte = part
                    .start_byte
                    .checked_add(u64::try_from(end_in_part - cursor).ok()?)?;
                self.surface
                    .run_text(part.run)?
                    .get(usize::try_from(start_byte).ok()?..usize::try_from(end_byte).ok()?)?;
                if !parts.is_empty() {
                    joins.push(selection.joins[index - 1].clone());
                }
                parts.push(OutputSlice {
                    run: part.run,
                    start_byte,
                    end_byte,
                });
            }
            cursor = end;
        }
        (cursor == logical.len() && !parts.is_empty()).then_some(TextSelection { parts, joins })
    }

    /// Map disjoint declaration names against the already materialized HEAD.
    /// Unlike the public single-range helper, this private path walks the
    /// native selection once for all names. `form` is the unmodified result
    /// of `selection_text(selection)` in the same lexical proof operation.
    pub(super) fn selection_subranges_from_form(
        &self,
        selection: &TextSelection,
        form: &str,
        ranges: &[Range<usize>],
    ) -> Option<Vec<TextSelection>> {
        if selection.joins.len() != selection.parts.len().saturating_sub(1) {
            return None;
        }
        let mut previous_end = 0usize;
        for range in ranges {
            if range.start < previous_end
                || range.start >= range.end
                || form.get(range.clone()).is_none()
            {
                return None;
            }
            previous_end = range.end;
        }
        let mut found = ranges
            .iter()
            .map(|_| TextSelection {
                parts: Vec::new(),
                joins: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut current = 0usize;
        let mut cursor = 0usize;
        for (index, part) in selection.parts.iter().enumerate() {
            if index != 0 {
                match &selection.joins[index - 1] {
                    TextJoin::DirectContact => {}
                    TextJoin::AuthoredSeparator(separator)
                    | TextJoin::GeneratedSeparator(separator) => {
                        let end = cursor.checked_add(separator.len())?;
                        if ranges
                            .get(current)
                            .is_some_and(|range| range.start < end && cursor < range.end)
                        {
                            return None;
                        }
                        cursor = end;
                    }
                    TextJoin::HardBoundary | TextJoin::Unknown => return None,
                }
            }
            let start_byte = usize::try_from(part.start_byte).ok()?;
            let end_byte = usize::try_from(part.end_byte).ok()?;
            let run_text = self.surface.run_text(part.run)?;
            let visible = run_text.get(start_byte..end_byte)?;
            let end = cursor.checked_add(visible.len())?;
            // The public mapper skips zero-width parts even in an otherwise
            // malformed mutable selection; never emit an empty output slice.
            if visible.is_empty() {
                continue;
            }
            while let Some(range) = ranges.get(current)
                && range.start < end
            {
                if range.end <= cursor {
                    return None;
                }
                let clip_start = range.start.max(cursor);
                let clip_end = range.end.min(end);
                let slice_start = start_byte.checked_add(clip_start - cursor)?;
                let slice_end = start_byte.checked_add(clip_end - cursor)?;
                run_text.get(slice_start..slice_end)?;
                let target = found.get_mut(current)?;
                if !target.parts.is_empty() {
                    target
                        .joins
                        .push(selection.joins.get(index.checked_sub(1)?)?.clone());
                }
                target.parts.push(OutputSlice {
                    run: part.run,
                    start_byte: u64::try_from(slice_start).ok()?,
                    end_byte: u64::try_from(slice_end).ok()?,
                });
                if range.end <= end {
                    current += 1;
                } else {
                    break;
                }
            }
            cursor = end;
        }
        (cursor == form.len()
            && current == ranges.len()
            && found.iter().all(|selection| !selection.parts.is_empty()))
        .then_some(found)
    }

    /// Read one complete surviving definition head without inferring bytes
    /// from neighboring rows, owners or unknown native joins.
    #[must_use]
    pub fn owner_complete_form(&self, owner: &OwnerMark) -> Option<String> {
        if !owner.has_complete_form() {
            return None;
        }
        let form = self.selection_text(&owner.head)?;
        (!form.trim().is_empty()).then_some(form)
    }
}
