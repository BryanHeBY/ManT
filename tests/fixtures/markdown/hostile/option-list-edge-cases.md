# Option list edge cases

- ``: empty code span first
- `-a` `--b` no delimiter at all
- `-c`, prose between, `--d`: mixed alias run

## Almost options

- `-x` — em dash description.
- `-y`: colon description.
- `not-an-option`: code without a leading dash.

## Separator abuse

- `-e`,,,///|||: repeated separators.
- `-f` , | / : spaced separators before the colon.

## Nested under options

- `-g`: description with nested list.
  - `-h`: nested option list item.
  - plain nested prose item.

## Empty and colon-only items

- :
- `:`:
- `-i`:
