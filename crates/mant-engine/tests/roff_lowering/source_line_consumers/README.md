# Source line consumer witnesses

The complete sources were run with the registered pristine mandoc before
writing expectations. `receipts.json` binds the source bytes to the profile
results, including the expected filled-TAB lint warnings. The raw outputs
and source hashes are kept in
`target/audits/shared-source-line-fix/consumer-oracle`.

- `tab.1` retains the same physical row around a raw TAB.
- `bo-space.1`, `bo-continued-space.1`, and `xo-space.1` place a source SP
  at the first enclosure child. The Bo generated prefix executes before
  that child and consumes an incoming continuation.
- `inset.1` and `ohang.1` share the internal HEAD break but differ on the
  HEAD to BODY handoff.
- `tag-head-space.1` keeps the internal HEAD text on one device row
  with a three-space local gap. Its field flush contributes two spaces
  before the single authored ASCII SP.
- The four `*-body-space.1` inputs cross tag and hang fields with an
  ordinary or continued HEAD. The first BODY source SP can retire the
  live HEAD row only when the continuation register permits it.
- `link-tab.1` and `link-space.1` keep the two source TEXT children within
  UR BODY. Only SP breaks. The native URI suffix has a separate text owner.

Terminal comparison uses exact row offsets and one occurrence of every
marker. Tests also check exact author whitespace where the format can
represent it and typed link/definition ownership. Terminal TAB-stop
columns and native left margins are device geometry.

The three empty-HEAD controls record the run-in field at a source SP or
an explicit break request. Native UTF-8 prints the two generated diagnostic
cells on their own row, while an empty inset HEAD contributes no such word.
Their raw profiles are in the repair audit's `diag-controls.json`.
