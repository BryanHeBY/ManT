# R01 native capture checkpoint

This is an intermediate measurement at `e3635c23`, not the P1/G1 exit.
The new `annotated` feature produced checked owned native rows and runs for
the four P0 decoded input hashes listed in [README.md](README.md). It did
not produce a Fixed IR document or exercise CLI/TUI consumers. Semantic
coverage was still marked unverified at this checkpoint.

The single-process test `annotated_real_pages` decoded each fixture before
timing `AnnotatedRenderer::render_bundle` at the default 78 columns. Its
surface bytes exclude filtered page headers and footers. The following are
observed values, not performance thresholds:

| Page | Input bytes | Surface bytes | Rows | Runs | Marks | Debug ms | Release ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| GCC | 1,621,846 | 1,437,094 | 31,888 | 91,294 | 19,863 | 461–478 | 201–202 |
| Git | 67,376 | 62,569 | 1,651 | 3,855 | 156 | 20 | 7–8 |
| Clang | 28,479 | 26,573 | 676 | 1,651 | 420 | 9 | 4 |
| rclone | 3,346,767 | 3,257,887 | 90,871 | 199,952 | 68,417 | 923–937 | 397–422 |

Two direct runs of the release test binary had peak RSS 116,832 KiB; one
direct debug run had 117,256 KiB. These are whole-process maxima across all
four sequential pages, including decoded bundles and Rust test machinery,
not per-page native allocations. The existing P0 old-path load numbers
include different work and must not be compared as if they were the same
pipeline. R01 still needs post-coverage measurements and stronger raw-stream
and final-cell differentials; G1 requires full native→IR→CLI/TUI timing and
content validation.

## Direct-selection checkpoint at `608434af`

The same four decoded fixtures were rerun after native owner/link selections
and TextJoin transfer were added. This is still a native-only R01 checkpoint,
not G1 or a CLI/TUI outcome. The test now logs the shared selection-part and
authored-join arenas so their scale remains visible. One direct run of each
test binary, after compilation, observed:

| Page | Rows | Runs | Marks | Selection parts | Join bytes | Debug ms | Release ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| GCC | 31,888 | 91,294 | 19,863 | 65,349 | 10,446 | 536–546 | 255–265 |
| Git | 1,651 | 3,855 | 156 | 2,625 | 413 | 21–22 | 10 |
| Clang | 676 | 1,651 | 420 | 1,132 | 194 | 10–11 | 5 |
| rclone | 90,871 | 199,952 | 70,759 | 119,792 | 10,551 | 1,072–1,096 | 519–536 |

Whole-process peak RSS across all four sequential pages was 141,388 KiB
for one direct release run and 141,756 KiB for one direct debug run. Compared
with `e3635c23`, mark and selection functionality changed, so these figures
do not isolate TextJoin cost; they do flag an increase to investigate during
G1's alternating, like-for-like performance comparison. The result still
has no Fixed IR or consumer path, and native coverage remains conservative.
