# C01 structured-rendering baseline

This directory freezes the functional and performance checkpoint used before
the native structured-rendering implementation begins. The design contract and
scope are in
[`native-structured-rendering-contract.md`](../../native-structured-rendering-contract.md).

The functional sample is the checked-in
`tests/fixtures/roff/entry-name-boundaries-man.1` file, SHA-256
`b3d172e9c0eb769985af02be8ae3aa915cd3c335a418c85f9983bdfed3d5f34b`.
Before these artifacts were recorded, the exact file was run through the
registered pristine oracle with UTF-8 output and a 78-column profile. The raw
oracle output is `entry-probe.oracle-utf8-78.txt`.

`wrap-oracle-six-cases.txt` records the ASCII and UTF-8 output for six exact
`.ll 18n` logical-connection probes under the same 78-column registered oracle.
They cover a consumed ordinary space, an authored breakable hyphen, `\:`, `\%`,
`\&-`, and `\~`. These outputs establish the upstream boundary only; they do
not claim that the not-yet-implemented collector preserves it.

The `entry-probe.dev-start.*` files preserve the two opening-dev outputs that
changed. The other `entry-probe.*` files are exact outputs from the C00b release
binary:

- direct uncoloured text;
- document Markdown;
- the compact v0.12 document JSON envelope;
- an all-entry outline JSON projection; and
- an explanation JSON projection for `-Wall`.

The opening `dev` binary at
`aa211f7345dd80acd4b8b056e0deef0c368e18c6` produced byte-identical text,
Markdown, and outline files, so those shared tracked files cover both revisions.
Its document and explanation JSON differed only in `producer.engine.version`,
changing from `cvs-20260911` to `cvs-20260920T122115Z` after C00b; both versions
are tracked. Preserved binaries and full working probes also remain below
`target/structured-rendering-baseline/` but are not repository artifacts.

`performance-raw.ndjson` contains three alternating rounds for opening `dev`
and C00b. Each probe invocation contributes seven timings. The summarized
ranges, medians, host/toolchain, binary hashes, fixture hash, and separate
peak-RSS observations are in `performance-summary.json`. These measurements
are evidence, not a fixed performance threshold.

Regenerate functional artifacts from the repository root with the C00b release
binary:

```sh
target/release/mant --input tests/fixtures/roff/entry-name-boundaries-man.1 \
  --format text --display direct --color never
target/release/mant --input tests/fixtures/roff/entry-name-boundaries-man.1 \
  --format markdown --display direct
target/release/mant --input tests/fixtures/roff/entry-name-boundaries-man.1 \
  --format json --compact
target/release/mant --input tests/fixtures/roff/entry-name-boundaries-man.1 \
  --outline --outline-entries all --format json --compact
target/release/mant --input tests/fixtures/roff/entry-name-boundaries-man.1 \
  --explain=-Wall --format json --compact
```

The tracked checksums are:

```text
ceabe8f13f6708e08395c2469aa1fee6836ee847d416582afb4d80b4cc5aa3dc  entry-probe.document.json
455e1805748ca10793511df687bc626a2f7adf328bb8f11c3759af65496dc265  entry-probe.dev-start.document.json
03a1daf89e5f28693197fa859f20cc1490c493acbc50814452e80c434d11d4a2  entry-probe.dev-start.explain.json
a582cbab4a1a0386c141585a1c5e35d582478ba4c03e207368749ed4c6cf1b98  entry-probe.explain.json
6ace3620f7957f085c211369e98586ba07bb95626ed34f7e8193d6e0310c5d9b  entry-probe.md
40acdffb6a680699526834e8d5ab663305d4edaf49829d408f8dd1a09fc3a0e1  entry-probe.oracle-utf8-78.txt
1862ab8aa187296336d579642ba9ad3514854458f7bcabd6307038c94360d1c0  entry-probe.outline.json
15f290d7a5f35046a897effeca44afaa89bc9798ba223ec9154dca3778fe89d1  entry-probe.text
e0da0c9652c19e77a6080d0b2ac5c764a56128d38c32bf8d196db617f6be085d  performance-raw.ndjson
2f32d6e5a15e509057e08225d07d2615c7a5eb16edce52f279f00b0f348de0d7  performance-summary.json
4959e969bd42086b83bfc0f2bfbc4d42028a202a07d7b9bb3a0cde106c5c8c0b  wrap-oracle-six-cases.txt
```
