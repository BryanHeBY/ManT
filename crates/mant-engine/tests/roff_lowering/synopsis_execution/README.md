# Synopsis source phases

`cases.json` is the immutable 146-configuration historical matrix. Its original
source bytes, five-profile receipts, owner facts and selected reading policies
remain unchanged.

`word_observations.json` corrects one bounded omission in that older observer:
20 exact sources lost a real automatic word cell at the beginning of the
post-declaration SUFFIX row. The old harness trimmed every row's leading ASCII
spaces, and separately required the original paragraph owner to contain only
`SUFFIX`. The pinned `term_word()` writes that cell after a direct
`term_flushln()`; only `term_newln()` selects NOSPACE. This is an observation
correction against the same CVS identity, not a new compatibility contract.

The correction binds the original fixture hash, case ID, full source, source
hash and all original profile hashes. All 20 full old sources independently ran
ASCII, UTF-8, HTML, tree and lint again before changing an assertion. The first
four profiles match the old receipts byte for byte. The new lint invocation has
its own unchanged diagnostic stream with a different input transport name; both
old and new raw receipts are kept, without rewriting diagnostics to force a
match. No product executable supplies these expectations.

For only these registered sources the consumer tests preserve every leading
word cell and use the independently recorded native/readback/owner expectations.
The existing Markdown paragraph boundaries are retained. Fn/Fo followed by a
real `.br`, spacing off, empty words and mode transitions have separate positive
and negative controls in `word_spacing`; source ownership and styles remain
covered here. Other historical configurations keep their original projection.

Reproduce or verify the correction with:

```sh
python3 -m scripts.roff.fixtures.record_synopsis_word_observations
python3 -m unittest scripts.roff.tests.test_synopsis_word_observations
```

Only `--write` updates the new observation fixture. It never modifies the old
matrix. `--evidence DIR` keeps all raw source/profile bytes in the requested
task-owned audit directory.

The UI's separate immutable 103-case source-execution cohort contains eight
identical direct Fn/Fo sources. Its local `word_observations.json` binds those
full sources and the old UI fixture hash to the same fresh five-profile runs.
Both owner checks and actual painted rows preserve the SUFFIX word cell by
removing only the shared PREFIX viewport origin. Packaged UI tests carry their
own observation file; they do not include another package's test sources.
