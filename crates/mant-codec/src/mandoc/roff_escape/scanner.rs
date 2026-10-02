//! Bounded scanner for nested escape arguments.
//!
//! CVS `roff_escape()` recursively parses nested escapes before testing an
//! outer delimiter.  We preserve that grammar with a bounded heap stack: one
//! valid input text node must never be able to overflow the Rust call stack.
//! That includes the quoted-argument rules of `roff_escape.c` (the
//! `term == '\b'` families): an argument may be delimited by a complete
//! escape — `\o\(aqXY\(aq` opens with the `\(aq` escape and closes at the
//! next `(`-triggered escape, whatever it displays — a literal delimiter
//! character only ends an argument that was opened literally, and `\N` ends
//! at its first non-digit.  If the shared native nesting budget is exceeded,
//! consuming the remainder is conservative because no later delimiter can be
//! proven to belong to the outer request.
use super::ASCII_HYPH;
use super::grammar::{ArgumentShape, EscapeKind, syntax};

const MAX_NESTED_ESCAPE_SCAN_DEPTH: usize = 256;

#[cfg(test)]
thread_local! {
    static SCAN_WORK: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
pub(super) fn take_scan_work() -> (usize, usize) {
    SCAN_WORK.with(|work| work.replace((0, 0)))
}

#[cfg(test)]
fn record_scan_work(before: usize, after: usize, task_depth: usize) {
    SCAN_WORK.with(|work| {
        let (units, depth) = work.get();
        work.set((
            units.saturating_add(before.abs_diff(after) + 1),
            depth.max(task_depth),
        ));
    });
}

/// CVS `roff_escape.c:292` rejects these quoted families when the delimiter is
/// written as one of the non-printing escape names.
const REJECT_ESCAPED_DELIMITER_OUTERS: &[char] =
    &['B', 'H', 'L', 'R', 'S', 'N', 'h', 'l', 'v', 'x'];
const REJECTED_DELIMITER_NAMES: &[char] = &[
    ' ', ',', '.', '0', 'D', 'L', 'O', 'X', 'Y', 'Z', '^', 'a', 'b', 'd', 'h', 'l', 'o', 'r', 't',
    'u', 'v', 'x', '|', '~',
];

/// CVS `roff_escape.c:303` rejects these quoted families when the delimiter is
/// a literal character that cannot delimit anything.  `\D` only warns
/// upstream and keeps scanning, so it is absent here.
const REJECT_LITERAL_DELIMITER_OUTERS: &[char] =
    &['B', 'H', 'L', 'R', 'S', 'v', 'x', 'N', 'h', 'l'];
const REJECTED_DELIMITER_LITERALS: &[char] = &[
    ' ', '%', '&', '(', ')', '*', '+', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6', '7', '8',
    '9', ':', '<', '=', '>',
];

/// CVS `roff_escape.c:349` keeps the scanned payload of an unterminated
/// argument only for these families; everyone else drops it.
const KEEP_UNCLOSED_PAYLOAD_OUTERS: &[char] = &['A', 'o', 'w'];

/// Position and trigger of the escape whose backslash sits at `start`,
/// skipping `\E` copies.  `None` when the backslash is the final character.
fn escape_trigger_at(characters: &[char], start: usize) -> Option<(usize, char)> {
    let mut index = start + 1;
    while characters.get(index) == Some(&'E') {
        index += 1;
    }
    characters.get(index).map(|trigger| (index, *trigger))
}

/// How a quoted argument's terminator was written.  The syntax decides the
/// ending rule, not the displayed character: `\(aq`, `\(dq` and `\[aq]`
/// display differently but all open and close on their trigger.
#[derive(Clone, Copy, PartialEq)]
enum Term {
    /// One raw character ends the argument (CVS `escterm == 0`).
    Literal(char),
    /// Only a nested escape with this trigger ends the argument
    /// (CVS `escterm == 1`).
    Escaped(char),
    /// The opening escape ran into the end of input; nothing can close it.
    Never,
}

#[derive(Clone, Copy)]
enum EscapeScanTask {
    /// Parse one complete escape starting at `index` (a backslash), leaving
    /// `index` just past it.
    Escape,
    /// Scan until a literal delimiter character, taking nested escapes into
    /// the payload without ending it.
    Until(char),
    /// Consume `count` argument units; nested escapes are consumed whole
    /// without counting (CVS `maxl` counting).
    Counted(usize),
    /// Opaque `[..]`, `(..)` or single-unit argument of an expand-class
    /// escape.
    Opaque,
    /// Size argument shapes of `\s`.
    Size,
    /// Decide the delimiter of a quoted argument; `index` is the delimiter
    /// position and `outer` the owning trigger.
    Delimited { outer: char, report: bool },
    /// Scan one decided quoted argument.
    Scan {
        outer: char,
        term: Term,
        payload_start: usize,
        /// CVS `iend`: everything proven consumed so far.  An unclosed
        /// argument ends here, so an unconsumed trailing delimiter stays
        /// available as document text.
        consumed_end: usize,
        report: bool,
    },
    /// The escape that opens a quoted argument finished; classify it and set
    /// up the payload scan (or reject the delimiter).
    OpeningEscape {
        outer: char,
        report: bool,
        delim_index: usize,
    },
    /// A nested escape inside a quoted payload finished; close on a matching
    /// escaped terminator, otherwise continue after it.
    NestedEscape {
        outer: char,
        term: Term,
        payload_start: usize,
        report: bool,
        nested_start: usize,
    },
}

/// Result of scanning one quoted argument for the caller that owns it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum QuotedOutcome {
    /// Ended at a real closing delimiter, or at `\N`'s first non-digit.
    Closed { payload: std::ops::Range<usize> },
    /// Input ended first.  Only the `Aow` families keep their payload.
    Unclosed {
        payload: Option<std::ops::Range<usize>>,
    },
    /// CVS rejects the delimiter itself (`ESCAPE_DELIM)`: the family renders
    /// nothing and the argument ends right after the rejected delimiter.
    Rejected { kind: EscapeKind },
    /// No delimiter was present at all.
    Missing,
}

enum ScanStep {
    /// Keep processing the task stack.
    Going,
    /// The one reported quoted argument finished; `index` is its end.
    Reported(QuotedOutcome),
}

/// One bounded scan over a character slice.
struct EscapeScan<'a> {
    characters: &'a [char],
    index: usize,
    tasks: Vec<EscapeScanTask>,
    /// `outer` and payload start of the reported frame once its opening
    /// delimiter has been decided, for the conservative overflow outcome.
    reported_scan: Option<(char, usize)>,
    /// Whether the most recently completed counted/delimited name reached
    /// its terminator.  Consumed extent and completeness are independent:
    /// an incomplete name may still consume the whole remaining input.
    argument_complete: bool,
}

/// Completion is independent of extent: an incomplete operand can consume
/// the remainder, while an empty complete operand consumes its terminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArgumentCompletion {
    Complete,
    Incomplete,
    Missing,
    Rejected,
    BudgetExceeded,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScannedArgument {
    pub(super) kind: EscapeKind,
    pub(super) payload: Option<std::ops::Range<usize>>,
    pub(super) prefix: std::ops::Range<usize>,
    pub(super) end: usize,
    pub(super) completion: ArgumentCompletion,
}

struct ArgumentPlan {
    payload: usize,
    prefix: std::ops::Range<usize>,
    task: Option<EscapeScanTask>,
    rejected: bool,
    /// Initial CVS iend: standard shape openers are not yet consumed.
    empty_end: usize,
}

/// Shared CVS argument shape switch: sizes use a fixed literal quote;
/// standard names have only one-unit, two-unit, and bracketed forms.
fn argument_plan(characters: &[char], start: usize, shape: ArgumentShape) -> ArgumentPlan {
    let mut index = start;
    let size = shape == ArgumentShape::Size;
    let signed = size && matches!(characters.get(index), Some('+' | '-' | &ASCII_HYPH));
    if signed {
        index += 1;
    }
    let prefix = start..index;
    let mut rejected = false;
    let task = match shape {
        ArgumentShape::None | ArgumentShape::Quoted => None,
        ArgumentShape::Counted(count) => Some(EscapeScanTask::Counted(count)),
        ArgumentShape::Bracketed => {
            if characters.get(index) == Some(&' ') {
                rejected = true;
                index += 1;
                None
            } else {
                Some(EscapeScanTask::Until(']'))
            }
        }
        ArgumentShape::Standard | ArgumentShape::Size => match characters.get(index) {
            Some('[') => {
                index += 1;
                if !size && characters.get(index) == Some(&' ') {
                    rejected = true;
                    index += 1;
                    None
                } else {
                    Some(EscapeScanTask::Until(']'))
                }
            }
            Some('(') => {
                index += 1;
                Some(EscapeScanTask::Counted(2))
            }
            Some('\'') if size => {
                index += 1;
                Some(EscapeScanTask::Until('\''))
            }
            Some('1' | '2' | '3')
                if size
                    && !signed
                    && characters.get(index + 1).is_some_and(char::is_ascii_digit) =>
            {
                Some(EscapeScanTask::Counted(2))
            }
            _ => Some(EscapeScanTask::Counted(1)),
        },
    };
    ArgumentPlan {
        payload: index,
        prefix,
        task,
        rejected,
        empty_end: if shape == ArgumentShape::Standard {
            start
        } else {
            index
        },
    }
}

/// Decode a non-quoted operand with exactly the nested scanner's shape and
/// counting rules. Payload bounds use scalar indices into the supplied slice.
pub(super) fn scan_argument(characters: &[char], start: usize, trigger: char) -> ScannedArgument {
    // A digit immediately after N is a rejected literal delimiter. Its
    // known recovery extent is exactly one unit (roff_escape.c ESC_DELIM),
    // not an ordinary quoted payload or a standard O operand.
    let numbered_digit = trigger == 'N' && characters.get(start).is_some_and(char::is_ascii_digit);
    let shape = if numbered_digit {
        ArgumentShape::Counted(1)
    } else {
        syntax(trigger).argument
    };
    let plan = argument_plan(characters, start, shape);
    let mut scan = EscapeScan {
        characters,
        index: plan.payload,
        tasks: plan.task.into_iter().collect(),
        reported_scan: None,
        argument_complete: true,
    };
    let budget_exhausted = scan.run_extent();
    let completion = if budget_exhausted {
        ArgumentCompletion::BudgetExceeded
    } else if plan.payload == characters.len() && shape != ArgumentShape::Standard {
        ArgumentCompletion::Missing
    } else if plan.rejected {
        ArgumentCompletion::Rejected
    } else if scan.argument_complete {
        ArgumentCompletion::Complete
    } else {
        ArgumentCompletion::Incomplete
    };
    // roff_escape.c's mandatory-argument check runs before scanning sizes,
    // but standard shape selection runs afterwards. An empty standard name
    // therefore leaves its opening bracket/parenthesis unconsumed.
    if plan.payload == characters.len() {
        scan.index = plan.empty_end;
    }
    let terminator = matches!(plan.task, Some(EscapeScanTask::Until(_)))
        && completion == ArgumentCompletion::Complete;
    let payload_end = scan
        .index
        .saturating_sub(usize::from(terminator))
        .max(plan.payload);
    // CVS post-processes O only after the complete standard operand was
    // consumed. Invalid operands remain known escapes, but produce ERROR,
    // not an IGNORE row cell. O0 and bracketed O5 are UNSUPP (no cell).
    let base_kind = completion_kind(syntax(trigger).kind, completion);
    let invalid_font = trigger == 'f'
        && !super::grammar::valid_font_operand(&characters[plan.payload..payload_end]);
    let kind = if base_kind == EscapeKind::Error || plan.rejected || numbered_digit || invalid_font
    {
        EscapeKind::Error
    } else if trigger == 'O' {
        match characters.get(plan.payload) {
            Some('0') => EscapeKind::Unsupported,
            Some('1' | '2' | '3' | '4') if payload_end == plan.payload + 1 => EscapeKind::Ignore,
            Some('5') if characters.get(plan.payload.saturating_sub(1)) == Some(&'[') => {
                EscapeKind::Unsupported
            }
            _ => EscapeKind::Error,
        }
    } else {
        base_kind
    };
    ScannedArgument {
        kind,
        payload: (!plan.rejected && completion != ArgumentCompletion::Missing)
            .then_some(plan.payload..payload_end),
        prefix: plan.prefix,
        end: scan.index,
        completion,
    }
}

/// Upstream mandatory EOF keeps its initialized class except SPECIAL;
/// failure after payload scanning keeps only EXPAND/OVERSTRIKE.
pub(super) fn completion_kind(kind: EscapeKind, completion: ArgumentCompletion) -> EscapeKind {
    match completion {
        ArgumentCompletion::Complete => kind,
        ArgumentCompletion::Missing if kind != EscapeKind::Special => kind,
        ArgumentCompletion::Incomplete
            if matches!(kind, EscapeKind::Expand | EscapeKind::Overstrike) =>
        {
            kind
        }
        _ => EscapeKind::Error,
    }
}

/// Scan the quoted argument of `outer` starting at its delimiter position.
/// Returns the outcome plus the index just past everything consumed.
pub(super) fn scan_quoted_argument(
    characters: &[char],
    start: usize,
    outer: char,
) -> (QuotedOutcome, usize, bool) {
    let mut scan = EscapeScan {
        characters,
        index: start,
        tasks: vec![EscapeScanTask::Delimited {
            outer,
            report: true,
        }],
        reported_scan: None,
        argument_complete: true,
    };
    loop {
        if scan.tasks.len() > MAX_NESTED_ESCAPE_SCAN_DEPTH {
            // Budget exhausted: behave like an unclosed argument that
            // consumes the remainder (see the module documentation).
            let payload = scan.reported_scan.and_then(|(outer, payload_start)| {
                keep_unclosed_payload(outer).then_some(payload_start..characters.len())
            });
            return (QuotedOutcome::Unclosed { payload }, characters.len(), true);
        }
        let Some(task) = scan.tasks.pop() else {
            // Every reported frame reports its own end before its task is
            // dropped, so an empty stack here is unreachable.
            return (
                QuotedOutcome::Unclosed { payload: None },
                characters.len(),
                false,
            );
        };
        #[cfg(test)]
        let before = scan.index;
        let result = scan.step(task);
        #[cfg(test)]
        record_scan_work(before, scan.index, scan.tasks.len());
        if let ScanStep::Reported(outcome) = result {
            return (outcome, scan.index, false);
        }
    }
}

fn keep_unclosed_payload(outer: char) -> bool {
    KEEP_UNCLOSED_PAYLOAD_OUTERS.contains(&outer)
}

impl EscapeScan<'_> {
    /// Run every task; on budget exhaustion consume the remainder.
    fn run_extent(&mut self) -> bool {
        while self.tasks.len() <= MAX_NESTED_ESCAPE_SCAN_DEPTH {
            let Some(task) = self.tasks.pop() else {
                return false;
            };
            #[cfg(test)]
            let before = self.index;
            let _ = self.step(task);
            #[cfg(test)]
            record_scan_work(before, self.index, self.tasks.len());
        }
        self.index = self.characters.len();
        true
    }

    fn step(&mut self, task: EscapeScanTask) -> ScanStep {
        match task {
            EscapeScanTask::Escape => self.step_escape(),
            EscapeScanTask::Until(delimiter) => self.step_until(delimiter),
            EscapeScanTask::Counted(count) => self.step_counted(count),
            EscapeScanTask::Opaque => self.step_opaque(),
            EscapeScanTask::Size => self.step_size(),
            EscapeScanTask::Delimited { outer, report } => self.decide_delimiter(outer, report),
            EscapeScanTask::Scan {
                outer,
                term,
                payload_start,
                consumed_end,
                report,
            } => self.scan_payload(outer, term, payload_start, consumed_end, report),
            EscapeScanTask::OpeningEscape {
                outer,
                report,
                delim_index,
            } => self.open_with_escape(outer, report, delim_index),
            EscapeScanTask::NestedEscape {
                outer,
                term,
                payload_start,
                report,
                nested_start,
            } => self.close_or_continue_nested(outer, term, payload_start, report, nested_start),
        }
    }

    fn step_escape(&mut self) -> ScanStep {
        if self.characters.get(self.index) != Some(&'\\') {
            return ScanStep::Going;
        }
        self.index += 1;
        while self.characters.get(self.index) == Some(&'E') {
            self.index += 1;
        }
        let Some(trigger) = self.characters.get(self.index).copied() else {
            return ScanStep::Going;
        };
        let trigger_index = self.index;
        self.index += 1;
        self.argument_complete = true;
        // CVS roff_escape_impl() initializes `iend` to the name trigger
        // for `(`/`[` and advances it only as name units are consumed.
        // An entirely absent name therefore leaves the trigger available;
        // a partial name still consumes every scanned unit.  In particular,
        // an unclosed `\[name` consumes the remainder, rather than rewinding
        // to `[` merely because it lacks the closing bracket.
        if matches!(trigger, '(' | '[') && self.index == self.characters.len() {
            self.index = trigger_index;
            self.argument_complete = false;
            return ScanStep::Going;
        }
        self.schedule_escape_argument(trigger);
        ScanStep::Going
    }

    fn step_until(&mut self, delimiter: char) -> ScanStep {
        loop {
            match self.characters.get(self.index).copied() {
                None => {
                    self.argument_complete = false;
                    break;
                }
                Some(character) if character == delimiter => {
                    self.index += 1;
                    self.argument_complete = true;
                    break;
                }
                // A nested escape belongs to the payload; only the matching
                // literal character ends it.
                Some('\\') => {
                    self.tasks.push(EscapeScanTask::Until(delimiter));
                    self.tasks.push(EscapeScanTask::Escape);
                    break;
                }
                Some(_) => self.index += 1,
            }
        }
        ScanStep::Going
    }

    fn step_counted(&mut self, mut remaining: usize) -> ScanStep {
        while remaining > 0 {
            match self.characters.get(self.index).copied() {
                // CVS maxl counting: the whole nested escape is consumed
                // without counting against the expected length.
                Some('\\') => {
                    self.tasks.push(EscapeScanTask::Counted(remaining));
                    self.tasks.push(EscapeScanTask::Escape);
                    break;
                }
                None => {
                    self.argument_complete = false;
                    break;
                }
                Some(_) => {
                    self.index += 1;
                    remaining -= 1;
                }
            }
        }
        if remaining == 0 {
            self.argument_complete = true;
        }
        ScanStep::Going
    }

    fn step_opaque(&mut self) -> ScanStep {
        self.schedule_shape(ArgumentShape::Standard)
    }

    fn step_size(&mut self) -> ScanStep {
        self.schedule_shape(ArgumentShape::Size)
    }

    fn schedule_shape(&mut self, shape: ArgumentShape) -> ScanStep {
        let plan = argument_plan(self.characters, self.index, shape);
        self.argument_complete = !plan.rejected;
        if plan.payload == self.characters.len() {
            self.index = plan.empty_end;
            self.argument_complete = false;
        } else {
            self.index = plan.payload;
            self.tasks.extend(plan.task);
        }
        ScanStep::Going
    }

    fn schedule_escape_argument(&mut self, trigger: char) {
        match syntax(trigger).argument {
            ArgumentShape::None => {}
            ArgumentShape::Counted(count) => self.tasks.push(EscapeScanTask::Counted(count)),
            ArgumentShape::Bracketed => {
                let _ = self.schedule_shape(ArgumentShape::Bracketed);
            }
            ArgumentShape::Standard => self.tasks.push(EscapeScanTask::Opaque),
            ArgumentShape::Size => self.tasks.push(EscapeScanTask::Size),
            ArgumentShape::Quoted
                if trigger == 'N'
                    && self
                        .characters
                        .get(self.index)
                        .is_some_and(char::is_ascii_digit) =>
            {
                self.index += 1;
            }
            ArgumentShape::Quoted => {
                self.tasks.push(EscapeScanTask::Delimited {
                    outer: trigger,
                    report: false,
                });
            }
        }
    }

    /// Decide how the quoted argument of `outer` is delimited
    /// (roff_escape.c:278-314).
    fn decide_delimiter(&mut self, outer: char, report: bool) -> ScanStep {
        let Some(character) = self.characters.get(self.index).copied() else {
            // Mandatory argument missing: nothing beyond the trigger is
            // consumed and the family renders nothing.
            return Self::reported(report, QuotedOutcome::Missing);
        };
        if character == '\\' {
            // The delimiter may itself be written as an escape; classify it
            // once the complete escape has been consumed.
            self.tasks.push(EscapeScanTask::OpeningEscape {
                outer,
                report,
                delim_index: self.index,
            });
            self.tasks.push(EscapeScanTask::Escape);
            return ScanStep::Going;
        }
        if REJECT_LITERAL_DELIMITER_OUTERS.contains(&outer)
            && REJECTED_DELIMITER_LITERALS.contains(&character)
        {
            self.index += 1;
            return Self::reported(
                report,
                QuotedOutcome::Rejected {
                    kind: EscapeKind::Error,
                },
            );
        }
        // Consume the delimiter character; the payload starts after it.  CVS
        // only counts the delimiter as consumed once payload followed
        // (`iend`), so an unclosed empty argument gives it back as text.
        self.index += 1;
        self.begin_scan(
            outer,
            Term::Literal(character),
            self.index,
            self.index - 1,
            report,
        )
    }

    /// The escape opening a quoted argument finished at `index`; adopt its
    /// trigger as the terminator (roff_escape.c:288-314).
    fn open_with_escape(&mut self, outer: char, report: bool, delim_index: usize) -> ScanStep {
        let Some((trigger_index, trigger)) = escape_trigger_at(self.characters, delim_index) else {
            // A trailing backslash parses as ESCAPE_UNDEF over the end of
            // input; no delimiter can follow it.
            return self.begin_scan(
                outer,
                Term::Never,
                self.characters.len(),
                delim_index,
                report,
            );
        };
        if syntax(trigger).kind == EscapeKind::Undefined {
            // ESCAPE_UNDEF: the backslash is dropped and the trigger
            // character itself becomes the literal delimiter — and, like a
            // literal delimiter, is still subject to the ESCAPE_DELIM
            // rejection of roff_escape.c:303-311 (upstream's `iarg++`
            // lands on buf[snam] before that check).  `index` already
            // sits just past the two-byte UNDEF spelling.
            if REJECT_LITERAL_DELIMITER_OUTERS.contains(&outer)
                && REJECTED_DELIMITER_LITERALS.contains(&trigger)
            {
                return Self::reported(
                    report,
                    QuotedOutcome::Rejected {
                        kind: EscapeKind::Error,
                    },
                );
            }
            // As with a literal delimiter, nothing is proven consumed past
            // the backslash until payload follows.
            return self.begin_scan(
                outer,
                Term::Literal(trigger),
                trigger_index + 1,
                delim_index,
                report,
            );
        }
        if REJECT_ESCAPED_DELIMITER_OUTERS.contains(&outer)
            && REJECTED_DELIMITER_NAMES.contains(&trigger)
        {
            // `index` already sits at the rejected opening escape's end.
            return Self::reported(
                report,
                QuotedOutcome::Rejected {
                    kind: syntax(outer).kind,
                },
            );
        }
        // The payload begins past the complete opening escape; only a nested
        // escape with the same trigger can close it.  The opening escape is
        // proven consumed only once payload follows: CVS keeps `iend` at the
        // delimiter's backslash until the scan loop advances it.  The nested
        // scanner's end is the actual CVS `send`, including partial names;
        // it is independent of whether the nested escape is valid.  When
        // that incomplete name consumes the remainder, its residual is an
        // invisible ESCAPE_ERROR upstream; do not re-decode it as a fallback
        // symbol. A complete nonempty delimiter at EOF remains available
        // as text; an empty bracketed name is CVS's invisible ESC_BADCHAR.
        let empty_bracketed_name = trigger == '['
            && self.characters.get(trigger_index + 1) == Some(&']')
            && self.index == trigger_index + 2;
        let consumed_end = if matches!(trigger, '(' | '[')
            && (!self.argument_complete || empty_bracketed_name)
            && self.index == self.characters.len()
        {
            self.index
        } else {
            delim_index
        };
        self.begin_scan(
            outer,
            Term::Escaped(trigger),
            self.index,
            consumed_end,
            report,
        )
    }

    /// Advance one quoted payload scan (roff_escape.c:342-380).
    fn scan_payload(
        &mut self,
        outer: char,
        term: Term,
        payload_start: usize,
        consumed_end: usize,
        report: bool,
    ) -> ScanStep {
        // CVS `iend` tracks every unit the scan loop has proven consumed.
        let mut consumed_end = consumed_end;
        loop {
            let Some(character) = self.characters.get(self.index).copied() else {
                // Input exhausted: only the Aow families keep their payload,
                // and only the proven-consumed prefix belongs to the escape
                // (CVS `iend`).
                let payload =
                    keep_unclosed_payload(outer).then_some(payload_start..self.characters.len());
                self.index = consumed_end;
                return Self::reported(report, QuotedOutcome::Unclosed { payload });
            };
            if let Term::Literal(delimiter) = term
                && character == delimiter
            {
                self.index += 1;
                let payload = payload_start..self.index - 1;
                return Self::reported(report, QuotedOutcome::Closed { payload });
            }
            if character == '\\' {
                if escape_trigger_at(self.characters, self.index).is_none() {
                    // A trailing backslash is ESCAPE_UNDEF and consumes to
                    // the end of input (`iend = send`); roff_escape.c:363
                    // still lets `\N` close on it, at the end of input.
                    consumed_end = self.characters.len();
                    let backslash = self.index;
                    self.index = self.characters.len();
                    if matches!(term, Term::Escaped(_)) && outer == 'N' {
                        let payload = payload_start..backslash;
                        return Self::reported(report, QuotedOutcome::Closed { payload });
                    }
                    continue;
                }
                self.tasks.push(EscapeScanTask::NestedEscape {
                    outer,
                    term,
                    payload_start,

                    report,
                    nested_start: self.index,
                });
                self.tasks.push(EscapeScanTask::Escape);
                return ScanStep::Going;
            }
            if outer == 'N' && !character.is_ascii_digit() {
                // roff_escape.c:369: one non-digit ends a numbered argument.
                self.index += 1;
                let payload = payload_start..self.index - 1;
                return Self::reported(report, QuotedOutcome::Closed { payload });
            }
            self.index += 1;
            consumed_end = self.index;
        }
    }

    /// A nested payload escape finished at `index` (roff_escape.c:357-368).
    /// The whole nested escape is proven consumed either way.
    fn close_or_continue_nested(
        &mut self,
        outer: char,
        term: Term,
        payload_start: usize,

        report: bool,
        nested_start: usize,
    ) -> ScanStep {
        if let (Term::Escaped(delimiter), Some((_, trigger))) =
            (term, escape_trigger_at(self.characters, nested_start))
        {
            // `buf[snam] == term || buf[inam] == 'N'`: an escaped-delimiter
            // argument ends at a nested escape with the same trigger, and a
            // numbered argument at any nested escape.  `iend = send` uses
            // its actual consumed extent even when the name is incomplete.
            if trigger == delimiter || outer == 'N' {
                let payload = payload_start..nested_start;
                return Self::reported(report, QuotedOutcome::Closed { payload });
            }
        }
        // The nested escape stays inside the payload; an ESCAPE_EXPAND inner
        // would return to the parser (unreachable in a text node), so like
        // CVS we simply continue past it here.
        self.tasks.push(EscapeScanTask::Scan {
            outer,
            term,
            payload_start,
            consumed_end: self.index,
            report,
        });
        ScanStep::Going
    }

    fn begin_scan(
        &mut self,
        outer: char,
        term: Term,
        payload_start: usize,
        consumed_end: usize,
        report: bool,
    ) -> ScanStep {
        if report {
            self.reported_scan = Some((outer, payload_start));
        }
        self.tasks.push(EscapeScanTask::Scan {
            outer,
            term,
            payload_start,
            consumed_end,
            report,
        });
        ScanStep::Going
    }

    fn reported(report: bool, outcome: QuotedOutcome) -> ScanStep {
        if report {
            ScanStep::Reported(outcome)
        } else {
            ScanStep::Going
        }
    }
}
