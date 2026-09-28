//! Bounded scanner for nested escape arguments.
use super::ASCII_HYPH;

const MAX_NESTED_ESCAPE_SCAN_DEPTH: usize = 256;

#[derive(Clone, Copy)]
enum EscapeScanTask {
    Escape,
    Until(char),
    Counted(usize),
    Opaque,
    Size,
    Delimited,
}

/// Return the end of one complete nested escape without recursive descent.
///
/// CVS `roff_escape()` recursively parses nested escapes before testing an
/// outer delimiter. We preserve that grammar but use a bounded heap stack:
/// one valid input text node must never be able to overflow the Rust call
/// stack. If the shared native nesting budget is exceeded, consuming the
/// remainder is conservative because no later delimiter can be proven to
/// belong to the outer request.
pub(super) fn scan_nested_escape_end(characters: &[char], start: usize) -> usize {
    let mut index = start;
    let mut tasks = vec![EscapeScanTask::Escape];
    while let Some(task) = tasks.pop() {
        if !advance_escape_scan_task(characters, &mut index, task, &mut tasks)
            || tasks.len() > MAX_NESTED_ESCAPE_SCAN_DEPTH
        {
            return characters.len();
        }
    }
    index
}

/// Execute one iterative equivalent of a recursive `roff_escape()` frame.
/// Returning false means the shared nesting budget was exhausted.
fn advance_escape_scan_task(
    characters: &[char],
    index: &mut usize,
    task: EscapeScanTask,
    tasks: &mut Vec<EscapeScanTask>,
) -> bool {
    match task {
        EscapeScanTask::Escape => {
            if characters.get(*index) != Some(&'\\') {
                return true;
            }
            *index += 1;
            while characters.get(*index) == Some(&'E') {
                *index += 1;
            }
            let Some(trigger) = characters.get(*index).copied() else {
                return true;
            };
            *index += 1;
            schedule_escape_argument(characters, index, trigger, tasks);
        }
        EscapeScanTask::Until(delimiter) => loop {
            match characters.get(*index).copied() {
                None => break,
                Some(character) if character == delimiter => {
                    *index += 1;
                    break;
                }
                Some('\\') => {
                    if tasks.len() + 2 > MAX_NESTED_ESCAPE_SCAN_DEPTH {
                        return false;
                    }
                    tasks.push(EscapeScanTask::Until(delimiter));
                    tasks.push(EscapeScanTask::Escape);
                    break;
                }
                Some(_) => *index += 1,
            }
        },
        EscapeScanTask::Counted(mut remaining) => {
            while remaining > 0 {
                match characters.get(*index).copied() {
                    None => break,
                    Some('\\') => {
                        if tasks.len() + 2 > MAX_NESTED_ESCAPE_SCAN_DEPTH {
                            return false;
                        }
                        tasks.push(EscapeScanTask::Counted(remaining));
                        tasks.push(EscapeScanTask::Escape);
                        break;
                    }
                    Some(_) => {
                        *index += 1;
                        remaining -= 1;
                    }
                }
            }
        }
        EscapeScanTask::Opaque => match characters.get(*index).copied() {
            Some('\\') => tasks.push(EscapeScanTask::Escape),
            Some('[') => {
                *index += 1;
                tasks.push(EscapeScanTask::Until(']'));
            }
            Some('(') => {
                *index += 1;
                tasks.push(EscapeScanTask::Counted(2));
            }
            Some(_) => *index += 1,
            None => {}
        },
        EscapeScanTask::Size => {
            let has_sign = matches!(characters.get(*index), Some('+' | '-' | &ASCII_HYPH));
            if has_sign {
                *index += 1;
            }
            match characters.get(*index).copied() {
                Some('\\') => tasks.push(EscapeScanTask::Escape),
                Some('[') => {
                    *index += 1;
                    tasks.push(EscapeScanTask::Until(']'));
                }
                Some('(') => {
                    *index += 1;
                    tasks.push(EscapeScanTask::Counted(2));
                }
                Some('\'') => tasks.push(EscapeScanTask::Delimited),
                Some('1' | '2' | '3')
                    if !has_sign
                        && characters.get(*index + 1).is_some_and(char::is_ascii_digit) =>
                {
                    *index = (*index + 2).min(characters.len());
                }
                Some(_) => *index += 1,
                None => {}
            }
        }
        EscapeScanTask::Delimited => {
            let Some(delimiter) = characters.get(*index).copied() else {
                return true;
            };
            *index += 1;
            tasks.push(EscapeScanTask::Until(delimiter));
        }
    }
    true
}

fn schedule_escape_argument(
    characters: &[char],
    index: &mut usize,
    trigger: char,
    tasks: &mut Vec<EscapeScanTask>,
) {
    match trigger {
        '(' => tasks.push(EscapeScanTask::Counted(2)),
        '[' => tasks.push(EscapeScanTask::Until(']')),
        '$' | '*' | 'F' | 'M' | 'O' | 'V' | 'Y' | 'f' | 'g' | 'k' | 'm' | 'n' => {
            tasks.push(EscapeScanTask::Opaque);
        }
        's' => tasks.push(EscapeScanTask::Size),
        'N' if characters.get(*index).is_some_and(char::is_ascii_digit) => {
            *index = (*index + 1).min(characters.len());
        }
        'A' | 'B' | 'C' | 'D' | 'H' | 'L' | 'N' | 'R' | 'S' | 'X' | 'Z' | 'b' | 'h' | 'l' | 'o'
        | 'v' | 'w' | 'x' => tasks.push(EscapeScanTask::Delimited),
        _ => {}
    }
}
