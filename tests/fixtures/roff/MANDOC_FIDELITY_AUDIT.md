# Mandoc reference fidelity audit

`MANDOC_FIDELITY_AUDIT.csv` contains the unchanged checkout-fixture subset of the historical
mandoc content fidelity ledger. Original distribution records and their renderer
identity, commands, provenance and review conclusions are preserved in
[mant-roff-corpus](https://github.com/BryanHeBY/mant-roff-corpus/tree/main/audits/mant-tracked/tests/fixtures/roff/MANDOC_FIDELITY_AUDIT.md).

Do not relabel these old rows as observations from the active CVS oracle.
New checkout fixtures use the paired `MANDOC_CVS_FIXTURE_FIDELITY_AUDIT.csv`
and `MANDOC_CVS_FIXTURE_LAYOUT_AUDIT.csv` supplements bound to the current
pristine registration. Actual rendering requires a verified reference binary,
locked pristine archive and matching attestation; static ledger checks do not
claim a fresh rendering.

The [coverage contract](AUDIT_COVERAGE.md) requires exact source hashes,
matching reference identities and paired comparable evidence. Review, timeout,
skipped or unsupported results must retain their actual status. The
[fixture guide](README.md) explains external corpus paths and the
[development guide](../../../docs/development.md#mandoc-reference-replay)
describes the bounded renderer execution contract.
