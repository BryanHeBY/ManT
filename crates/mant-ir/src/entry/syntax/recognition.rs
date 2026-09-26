//! One bounded option-head decision over checked, final visible evidence.
//!
//! The view borrows a complete UTF-8 HEAD. Producers supply style and native
//! operand intervals in that HEAD's byte coordinates; the grammar never
//! interprets roff tokens or terminal cells. The resulting ranges still need
//! to be bound to the producer's original content before becoming entry facts.

use std::ops::Range;

use super::literal_names::{
    DeclarationScan, ItalicStartMode, StyledBoundaryRule, checked_plain_italic_starts,
    literal_declaration_scan_with_operands, scan_option_declarations_with_style_ranges,
    valid_evidence,
};

/// Borrowed final HEAD and independently checked option evidence.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct DeclarationView<'a> {
    /// Complete, final visible UTF-8 HEAD, not an authored roff operand.
    pub visible: &'a str,
    /// Full visible ranges of independent native macro operands.
    pub native_operands: &'a [Range<usize>],
    /// Final plain-italic argument ranges.
    pub plain_italic_ranges: &'a [Range<usize>],
    /// First glyphs of final bold-underlined ranges.
    pub bold_underline_starts: &'a [usize],
    /// First glyphs of independently proved numeric short option names.
    pub numeric_name_starts: &'a [usize],
}

/// Which operand boundary the producer can actually prove.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationContext {
    /// A sole text label has no independent native restart boundary.
    SingleTextOperand,
    /// The native HEAD has macro components, including textless components.
    NativeComponents,
}

/// One complete option scan. An empty name set is distinct from invalid
/// evidence, and exceeding the bounded name count is not a partial success.
#[doc(hidden)]
pub struct RecognitionResult {
    scan: DeclarationScan,
    names: Vec<(String, Range<usize>)>,
    over_limit: bool,
}

/// Owned, checked declaration ranges and names transferred together.
#[doc(hidden)]
pub struct RecognitionParts {
    pub ranges: Vec<Range<usize>>,
    pub names: Vec<(String, Range<usize>)>,
    pub over_limit: bool,
}

impl RecognitionResult {
    /// Complete, untrimmed declaration intervals in visible UTF-8 bytes.
    #[must_use]
    pub fn ranges(&self) -> &[Range<usize>] {
        self.scan.ranges()
    }

    /// Exact ordered option occurrences in visible UTF-8 byte coordinates.
    /// Empty after `over_limit` is not a proved non-option HEAD.
    #[must_use]
    pub fn names(&self) -> &[(String, Range<usize>)] {
        &self.names
    }

    /// Whether the complete head exceeded the existing 64-name budget.
    #[must_use]
    pub const fn over_limit(&self) -> bool {
        self.over_limit
    }

    /// Transfer checked ranges and names without copying a long HEAD's
    /// declaration intervals or every recognized spelling.
    #[must_use]
    pub fn into_parts(self) -> RecognitionParts {
        RecognitionParts {
            ranges: self.scan.ranges,
            names: self.names,
            over_limit: self.over_limit,
        }
    }
}

/// Recognize one complete option HEAD without binding it to a content owner.
/// `None` means contradictory byte/style/operand evidence, never no name.
#[doc(hidden)]
#[must_use]
pub fn recognize_option_declarations(
    view: DeclarationView<'_>,
    context: DeclarationContext,
) -> Option<RecognitionResult> {
    let scan = if context == DeclarationContext::NativeComponents && view.native_operands.is_empty()
    {
        // A lexical native HEAD may contain only non-lexical or textless
        // components. Preserve its conservative native boundary rule instead
        // of treating font runs as separate source operands.
        let mut argument_starts = checked_plain_italic_starts(
            view.visible,
            view.plain_italic_ranges,
            ItalicStartMode::ExactRangeStart,
        )?;
        argument_starts.extend_from_slice(view.bold_underline_starts);
        argument_starts.sort_unstable();
        argument_starts.dedup();
        if !view.numeric_name_starts.is_empty()
            || !valid_evidence(
                view.visible,
                view.native_operands,
                &argument_starts,
                view.bold_underline_starts,
                view.numeric_name_starts,
            )
        {
            return None;
        }
        literal_declaration_scan_with_operands(
            view.visible,
            view.native_operands,
            &argument_starts,
            StyledBoundaryRule::NativeComponents,
        )
    } else {
        scan_option_declarations_with_style_ranges(
            view.visible,
            view.native_operands,
            view.plain_italic_ranges,
            view.bold_underline_starts,
            view.numeric_name_starts,
        )?
    };
    let (mut names, over_limit) = scan.names(view.visible);
    if over_limit {
        names.clear();
    }
    Some(RecognitionResult {
        scan,
        names,
        over_limit,
    })
}

#[cfg(test)]
#[expect(
    clippy::single_range_in_vec_init,
    reason = "single Range values are checked byte-interval evidence, not iterators"
)]
mod tests {
    use std::ops::Range;

    use super::{
        DeclarationContext, DeclarationView, StyledBoundaryRule,
        literal_declaration_scan_with_operands, recognize_option_declarations,
        scan_option_declarations_with_style_ranges,
    };

    struct ScanCase<'a> {
        form: &'a str,
        operands: &'a [Range<usize>],
        italic: &'a [Range<usize>],
        bold_underline: &'a [usize],
        numeric: &'a [usize],
    }

    #[test]
    fn borrowed_view_matches_the_existing_complete_head_scan() {
        // Differential API test: these are not new roff expectations. The
        // original one-pass scanner remains the selected option grammar.
        let cases = [
            ScanCase {
                form: "-L first,--fake,last, --all FILE",
                operands: &[],
                italic: &[],
                bold_underline: &[],
                numeric: &[],
            },
            ScanCase {
                form: "-o\u{a0}--output FILE",
                operands: &[],
                italic: &[12..16],
                bold_underline: &[],
                numeric: &[],
            },
            ScanCase {
                form: "--pattern \"first,--fake,last\", --all",
                operands: &[],
                italic: &[],
                bold_underline: &[],
                numeric: &[],
            },
            ScanCase {
                form: "-4, --ipv4",
                operands: &[0..2, 2..4, 4..10],
                italic: &[],
                bold_underline: &[],
                numeric: &[0],
            },
            ScanCase {
                form: "-o FILE, --all FILE",
                operands: &[0..3, 3..7, 7..15, 15..19],
                italic: &[3..7, 15..19],
                bold_underline: &[],
                numeric: &[],
            },
        ];
        for ScanCase {
            form,
            operands,
            italic,
            bold_underline,
            numeric,
        } in cases
        {
            let old = scan_option_declarations_with_style_ranges(
                form,
                operands,
                italic,
                bold_underline,
                numeric,
            );
            let current = recognize_option_declarations(
                DeclarationView {
                    visible: form,
                    native_operands: operands,
                    plain_italic_ranges: italic,
                    bold_underline_starts: bold_underline,
                    numeric_name_starts: numeric,
                },
                if operands.is_empty() {
                    DeclarationContext::SingleTextOperand
                } else {
                    DeclarationContext::NativeComponents
                },
            );
            match (old, current) {
                (None, None) => {}
                (Some(old), Some(current)) => {
                    assert_eq!(current.ranges(), old.ranges());
                    let (mut names, over_limit) = old.names(form);
                    if over_limit {
                        names.clear();
                    }
                    assert_eq!(current.names(), names);
                    assert_eq!(current.over_limit(), over_limit);
                }
                _ => panic!("borrowed and original option scans disagreed: {form}"),
            }
        }
    }

    #[test]
    fn invalid_evidence_and_over_limit_do_not_become_empty_success() {
        let form = "-o\u{a0}--output";
        assert!(
            recognize_option_declarations(
                DeclarationView {
                    visible: form,
                    native_operands: &[1..3],
                    plain_italic_ranges: &[],
                    bold_underline_starts: &[],
                    numeric_name_starts: &[],
                },
                DeclarationContext::NativeComponents,
            )
            .is_none()
        );
        let form = (0..65).map(|_| "--many").collect::<Vec<_>>().join(", ");
        let result = recognize_option_declarations(
            DeclarationView {
                visible: &form,
                native_operands: &[],
                plain_italic_ranges: &[],
                bold_underline_starts: &[],
                numeric_name_starts: &[],
            },
            DeclarationContext::SingleTextOperand,
        )
        .expect("valid UTF-8 head");
        assert!(result.over_limit());
        assert!(result.names().is_empty());
    }

    #[test]
    fn textless_native_components_check_every_byte_evidence_interval() {
        // Differential internal boundary test: this is the conservative
        // no-lexical-operand rule already used by Fixed, not a new roff
        // expectation. pre_alternate() supplies operand identity, while
        // term_word() may change final font without a visible glyph.
        let form = "-o\u{a0}--output";
        let checked = |italic: &[Range<usize>], bold: &[usize], numeric: &[usize]| {
            recognize_option_declarations(
                DeclarationView {
                    visible: form,
                    native_operands: &[],
                    plain_italic_ranges: italic,
                    bold_underline_starts: bold,
                    numeric_name_starts: numeric,
                },
                DeclarationContext::NativeComponents,
            )
        };
        let valid = checked(&[4..12], &[], &[]).expect("valid native-style intervals");
        let old = literal_declaration_scan_with_operands(
            form,
            &[],
            &[4],
            StyledBoundaryRule::NativeComponents,
        );
        assert_eq!(valid.ranges(), old.ranges());
        assert_eq!(valid.names(), old.names(form).0);
        for italic in [&[3..5][..], &[4..20], &[4..4], &[0..5, 4..12]] {
            assert!(checked(italic, &[], &[]).is_none(), "{italic:?}");
        }
        for bold in [&[3][..], &[form.len()], &[4, 4]] {
            assert!(checked(&[], bold, &[]).is_none(), "{bold:?}");
        }
        // Without a visible native lexical operand, no numeric short-name
        // role can have proved this exception. Neither malformed nor merely
        // plausible numeric hints may revive a name.
        for numeric in [&[3][..], &[form.len()], &[0]] {
            assert!(checked(&[], &[], numeric).is_none(), "{numeric:?}");
        }
    }

    #[test]
    fn textless_native_components_preserve_whitespace_italic_start() {
        // This textless-component path historically passes range.start to
        // StyledArgument, even when the entire italic run is whitespace.
        // Keep its scanner decision while adding byte-evidence validation.
        let form = "--foo  , --bar";
        let italic = [5..7];
        let current = recognize_option_declarations(
            DeclarationView {
                visible: form,
                native_operands: &[],
                plain_italic_ranges: &italic,
                bold_underline_starts: &[],
                numeric_name_starts: &[],
            },
            DeclarationContext::NativeComponents,
        )
        .expect("valid whitespace-only italic interval");
        let old = literal_declaration_scan_with_operands(
            form,
            &[],
            &[italic[0].start],
            StyledBoundaryRule::NativeComponents,
        );
        assert_eq!(current.ranges(), old.ranges());
        assert_eq!(current.names(), old.names(form).0);
    }
}
