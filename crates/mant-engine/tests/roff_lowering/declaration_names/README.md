# Declaration name fixtures

`cases.json` records 102 accepted-head/name cases, `parameter_cases.json` records
78 parameter-boundary cases and a separate two-owner source, and `limits.json`
covers bounded declaration groups. Their expectations are kept independently of
product output.

`token_cases.json` adds 302 complete TP/HP sources:

- 234 positives: TP/HP × Roman/italic/bold × thirteen parameter spellings ×
  comma with space/comma without space/pipe.
- 68 negatives: quotes and four bracket kinds in all three fonts; comma-bearing
  opaque italic values; unproved literal/bold suffixes containing `--fake`;
  digit- or underscore-prefixed uppercase words without restart evidence.

Spellings include `script`, `Script`, `sCript`, `SCRIPT`, `ScriptFile`,
`Script-file`, `_script`, `_Script`, `SCRIPT_FILE`, `脚本`, `s_cript`, `1A`
and `_SCRIPT`.
Positive cases explicitly declare `-e` and `--expression` with one description.
Negative cases never name `--fake`: native TP tags can retain `-e`, while
unproved HP heads do not receive an inferred owner.

Every added source ran through the registered pristine reference in ASCII,
UTF-8, HTML, tree and lint before the assertions were written. All 302 fixed-date
sources pass lint. The registry binary SHA-256 is
`482cf7950a13b0aea4741d8cc7ed5e411435c7f4fcc1923c8cf29b5bf05accb6`.
Each record retains source and five-profile hashes plus UTF-8 body rows. Rows
resolve only font overstrikes and remove the common five-column body margin
and fixed footer furniture; internal spaces and blank rows remain intact.

Native evidence follows `man_term.c::pre_TP`, `pre_HP`, `pre_IP`,
`print_man_node` and `term.c::term_word`: in-word font changes preserve the
text without inventing a native operand boundary. The declaration names and
owner admission are ManT's authored semantic contract, not names inferred from
mandoc output. Tests validate actual TP HEAD or HP/IP AST ownership, original
name bindings, direct explanation/name evidence, visible search and excerpt
ranges, complete native body rows, and real query JSON/Markdown roundtrips.

Regenerate native evidence only from each record's complete `source` after
running `scripts/rebuild_reference_mandoc.sh`; use the five recorded profiles.
Keep authored names/exclusions and prior fixture gold separate from oracle
rendering observations. Product output must not supply native expectations.
