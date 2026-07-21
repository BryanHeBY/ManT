# Patch regression tests

Each `*.roff` file in this directory is a minimal roff input that
reproduces a single parser bug addressed by a corresponding C patch
under `patches/`.  The naming convention is `NNNN.descriptive-tag.roff`
to match the patch number.

Tests are run by the CI sync-libmandoc-vendor step: after applying
patches, it feeds each reproducer through the built libmandoc parser
and checks that the output matches expectations.

Currently empty – no C-level patches exist yet.
