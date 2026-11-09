# Native OP arity recovery

The 45 cases in `man_option_arity.json` retain exact source and SHA-256, all
five profile receipts, the number of operands kept in the native tree, and
the UTF-8 body after fixed five-column margin and overstrike processing.
Fifteen operand arrangements are crossed with an ordinary follower, raw
leading SP, and a UR label. They cover zero through four operands, the
legal and excess forms of both empty slots, and ignored operands with
continuation, zero advance, font selection and a break marker.

`man_validate.c::post_OP` diagnoses an excess operand without deleting it
from the tree. `man_term.c::pre_OP` still executes only the first two
`term_word` calls between its generated brackets. The engine test checks
exact body rows, source diagnostics, visible search, fonts and link targets
against the legal prefix, JSON/ANSI, and two real Markdown reads under each
export configuration. The TUI uses the ignored zero-advance and following
link case at 40 and 78 columns, including both hit edges and selection.
