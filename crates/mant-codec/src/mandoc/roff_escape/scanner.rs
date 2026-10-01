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

const MAX_NESTED_ESCAPE_SCAN_DEPTH: usize = 256;

/// Escape triggers recognized by the fixed CVS name switch.  Any other
/// trigger is `ESCAPE_UNDEF`: the backslash is dropped and the trigger
/// character itself becomes a literal delimiter.
const NAMED_TRIGGERS: &[char] = &[
    '!', '?', 'r', //
    '%', '&', ')', ',', '/', '^', 'a', 'd', 't', 'u', '{', '|', '}', //
    ' ', '\'', '-', '0', ':', '_', '`', 'e', '~', //
    'p', 'c', 'z', //
    '(', '[', 'f', 's', //
    'C', 'N', 'h', 'l', 'o', 'D', 'H', 'L', 'R', 'S', 'X', 'Z', 'b', 'v', 'x',
];

/// Escape triggers the roff parser expands away before a text node exists
/// (`\*`, `\n`, `\A`, `\B`, `\w`, ...).  CVS returns such an inner escape to
/// the parser for re-parsing (`ESCAPE_EXPAND`); fully parsed text nodes no
/// longer contain them, so the scanner merely gives them a bounded extent.
const EXPAND_TRIGGERS: &[char] = &['$', '*', 'A', 'B', 'V', 'g', 'n', 'w'];

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
    Rejected,
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

/// Return the end of one complete nested escape without recursive descent.
pub(super) fn scan_nested_escape_end(characters: &[char], start: usize) -> usize {
    let mut scan = EscapeScan {
        characters,
        index: start,
        tasks: vec![EscapeScanTask::Escape],
        reported_scan: None,
        argument_complete: true,
    };
    scan.run_extent();
    scan.index
}

/// End of a counted argument of `count` units, consuming nested escapes
/// whole without counting them (CVS `maxl` counting).
pub(super) fn scan_counted_end(characters: &[char], start: usize, count: usize) -> usize {
    let mut scan = EscapeScan {
        characters,
        index: start,
        tasks: vec![EscapeScanTask::Counted(count)],
        reported_scan: None,
        argument_complete: true,
    };
    scan.run_extent();
    scan.index
}

/// Scan the quoted argument of `outer` starting at its delimiter position.
/// Returns the outcome plus the index just past everything consumed.
pub(super) fn scan_quoted_argument(
    characters: &[char],
    start: usize,
    outer: char,
) -> (QuotedOutcome, usize) {
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
            return (QuotedOutcome::Unclosed { payload }, characters.len());
        }
        let Some(task) = scan.tasks.pop() else {
            // Every reported frame reports its own end before its task is
            // dropped, so an empty stack here is unreachable.
            return (QuotedOutcome::Unclosed { payload: None }, characters.len());
        };
        if let ScanStep::Reported(outcome) = scan.step(task) {
            return (outcome, scan.index);
        }
    }
}

fn keep_unclosed_payload(outer: char) -> bool {
    KEEP_UNCLOSED_PAYLOAD_OUTERS.contains(&outer)
}

impl EscapeScan<'_> {
    /// Run every task; on budget exhaustion consume the remainder.
    fn run_extent(&mut self) {
        while self.tasks.len() <= MAX_NESTED_ESCAPE_SCAN_DEPTH {
            let Some(task) = self.tasks.pop() else {
                return;
            };
            let _ = self.step(task);
        }
        self.index = self.characters.len();
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
        if trigger == '[' && self.characters.get(self.index) == Some(&' ') {
            // The standard-argument shape switch rejects a bracketed name
            // beginning with blank immediately (CVS ESC_ARG). Its `send`
            // includes that blank, but none of the later name-like text.
            self.index += 1;
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
        match self.characters.get(self.index).copied() {
            // CVS maxl counting skips the nested escape and still expects
            // the undecided single-unit shape's own unit afterwards.
            Some('\\') => {
                self.tasks.push(EscapeScanTask::Counted(1));
                self.tasks.push(EscapeScanTask::Escape);
            }
            Some('[') => {
                self.index += 1;
                self.tasks.push(EscapeScanTask::Until(']'));
            }
            Some('(') => {
                self.index += 1;
                self.tasks.push(EscapeScanTask::Counted(2));
            }
            Some(_) => self.index += 1,
            None => {}
        }
        ScanStep::Going
    }

    fn step_size(&mut self) -> ScanStep {
        let has_sign = matches!(
            self.characters.get(self.index),
            Some('+' | '-' | &ASCII_HYPH)
        );
        if has_sign {
            self.index += 1;
        }
        match self.characters.get(self.index).copied() {
            // Only the undecided single-unit shape can reach a backslash;
            // resume by counting, not by re-deciding the size shape.
            Some('\\') => {
                self.tasks.push(EscapeScanTask::Counted(1));
                self.tasks.push(EscapeScanTask::Escape);
            }
            Some('[') => {
                self.index += 1;
                self.tasks.push(EscapeScanTask::Until(']'));
            }
            Some('(') => {
                self.index += 1;
                self.tasks.push(EscapeScanTask::Counted(2));
            }
            // `\s'...'` is a fixed literal quote upstream: no escaped
            // delimiter syntax exists for sizes.
            Some('\'') => self.tasks.push(EscapeScanTask::Until('\'')),
            Some('1' | '2' | '3')
                if !has_sign
                    && self
                        .characters
                        .get(self.index + 1)
                        .is_some_and(char::is_ascii_digit) =>
            {
                self.index = (self.index + 2).min(self.characters.len());
            }
            Some(_) => self.index += 1,
            None => {}
        }
        ScanStep::Going
    }

    fn schedule_escape_argument(&mut self, trigger: char) {
        match trigger {
            '(' => self.tasks.push(EscapeScanTask::Counted(2)),
            '[' => self.tasks.push(EscapeScanTask::Until(']')),
            '$' | '*' | 'F' | 'M' | 'O' | 'V' | 'Y' | 'f' | 'g' | 'k' | 'm' | 'n' => {
                self.tasks.push(EscapeScanTask::Opaque);
            }
            's' => self.tasks.push(EscapeScanTask::Size),
            'N' if self
                .characters
                .get(self.index)
                .is_some_and(char::is_ascii_digit) =>
            {
                // The digit form ends at the first non-digit; CVS rejects a
                // digit delimiter right here, consuming exactly one unit.
                self.index = (self.index + 1).min(self.characters.len());
            }
            'A' | 'B' | 'C' | 'D' | 'H' | 'L' | 'N' | 'R' | 'S' | 'X' | 'Z' | 'b' | 'h' | 'l'
            | 'o' | 'v' | 'w' | 'x' => {
                self.tasks.push(EscapeScanTask::Delimited {
                    outer: trigger,
                    report: false,
                });
            }
            _ => {}
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
            return Self::reported(report, QuotedOutcome::Rejected);
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
        let named = NAMED_TRIGGERS.contains(&trigger) || EXPAND_TRIGGERS.contains(&trigger);
        if !named {
            // ESCAPE_UNDEF: the backslash is dropped and the trigger
            // character itself becomes the literal delimiter — and, like a
            // literal delimiter, is still subject to the ESCAPE_DELIM
            // rejection of roff_escape.c:303-311 (upstream's `iarg++`
            // lands on buf[snam] before that check).  `index` already
            // sits just past the two-byte UNDEF spelling.
            if REJECT_LITERAL_DELIMITER_OUTERS.contains(&outer)
                && REJECTED_DELIMITER_LITERALS.contains(&trigger)
            {
                return Self::reported(report, QuotedOutcome::Rejected);
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
            return Self::reported(report, QuotedOutcome::Rejected);
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
