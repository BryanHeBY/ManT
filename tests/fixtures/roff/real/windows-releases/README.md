# Official Windows release fixtures

These fixtures are exact roff members from official upstream Windows release
ZIPs, losslessly recompressed with zstd. They answer a narrower question than
the distribution corpora: whether the manuals that Rust and Go CLI projects
actually ship beside their Windows executables parse correctly on ManT's
native Windows path.

The decompressed bytes retain the release archives' line endings. `rg.1` has
mixed CRLF/LF terminators; `rclone.1` uses CRLF throughout.

| Fixture | Upstream release | ZIP member | Storage | Fixture license | Fixture SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `rg.1.zst` | [ripgrep 15.2.0], `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip` | `ripgrep-15.2.0-x86_64-pc-windows-msvc/doc/rg.1` | Lossless zstd recompression | [MIT] OR [Unlicense] | `d3c33d6550c1ceec1f5dd0ef4d56c0e2e5fc4d64fe6200f04b8527ee1fd5b983` |
| `rclone.1.zst` | [rclone v1.75.0], `rclone-v1.75.0-windows-amd64.zip` | `rclone-v1.75.0-windows-amd64/rclone.1` | Lossless zstd recompression | [rclone MIT] | `4e38c8e1a35e13faafda1d8bfa14b25f792e665476b7405d541e8462e049286e` |

The archives were retrieved on 2026-08-11 and have these SHA-256 values:

- `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip`: `71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5`
- `rclone-v1.75.0-windows-amd64.zip`: `203581f0a7baeae873f2347483a798c79e2eaf5c384a4e9d866aa374f1c89ac0`

Before recompression, the archive members have these SHA-256 values:

- `rg.1`: `e83d82c28bb2683cf71500e5fb8d9746a162bb24b08a6396a15ab2ed1d96cb45`
- `rclone.1`: `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e`

The fixtures were produced with Zstandard CLI 1.5.7 and `zstd -19`. This
changes only the container; decoding reproduces the hashes above exactly.

## License mapping

The ripgrep Windows ZIP ships both `LICENSE-MIT` and `UNLICENSE`; their complete
texts are retained as [`RIPGREP-LICENSE-MIT.txt`] and
[`RIPGREP-UNLICENSE.txt`]. The rclone Windows ZIP does not include its standalone
`COPYING` file, but `rclone.1` embeds a complete MIT grant in its `License`
section. The matching v1.75.0 source tag's `COPYING` is additionally retained
as [`RCLONE-COPYING.txt`]. Repository text normalization changes only these
license files' line endings to LF and leaves their wording unchanged. The
rclone file's 2012 copyright line differs from the
manual's embedded 2019 line; both upstream notices remain available rather than
being rewritten into a generic MIT template.

These notices apply only to the third-party fixture bytes. They do not change
ManT's repository-level Apache-2.0 license.

## Reproducing the fixtures

Download the exact release archives, extract the named members without newline
conversion, and recompress them:

```sh
unzip -p ripgrep-15.2.0-x86_64-pc-windows-msvc.zip \
  ripgrep-15.2.0-x86_64-pc-windows-msvc/doc/rg.1 > rg.1
zstd -19 -f -o rg.1.zst rg.1

unzip -p rclone-v1.75.0-windows-amd64.zip \
  rclone-v1.75.0-windows-amd64/rclone.1 > rclone.1
zstd -19 -f -o rclone.1.zst rclone.1
```

Verify both the decoded member hash and the checked-in zstd hash. Recompression
with another zstd version may produce different container bytes while still
decoding to the required raw hash.

## Observed parser behaviour

Both pages complete the bundled libmandoc pipeline. The ripgrep page yields 14
top-level sections and 104 semantic option entries. The much larger
Pandoc-generated rclone page yields 191 top-level and 3,430 total section nodes;
it also emits many diagnostics for Pandoc's GNU font extensions such as
`\f[V]`. Those diagnostics are not represented as a zero-warning claim in
`VERIFIED_TOPICS.txt`.

Manual review covered the document start and end, ripgrep's grouped options and
`--glob` excerpt, rclone's synopsis, PowerShell completion, global flags,
Microsoft OneDrive, Local Filesystem Windows paths, license, and authors. The
corresponding integration tests pin the stable structure and the Windows path
text that the review found most valuable.

The neighbouring [`VERIFIED_TOPICS.txt`] uses the same broader-scan convention
as the Linux distribution corpora. On 2026-08-11, all **9 manual pages** from
these **4 official Windows release archives** completed the same parser path:

| Release archive | Language | Pages scanned | Checked-in representative |
| --- | --- | ---: | --- |
| `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip` | Rust | 1 | `rg.1.zst` |
| `fd-v10.4.2-x86_64-pc-windows-msvc.zip` ([fd v10.4.2]) | Rust | 1 | — |
| `zoxide-0.10.0-x86_64-pc-windows-msvc.zip` ([zoxide v0.10.0]) | Rust | 6 | — |
| `rclone-v1.75.0-windows-amd64.zip` | Go | 1 | `rclone.1.zst` |

The fd archive SHA-256 is
`b2816e506390a89941c63c9187d58a3cc10e9a55f2ef0685f9ea0eccaf7c98c8`; the
zoxide archive SHA-256 is
`f465ae548f8754c8e7edbc60b45fbf58c92bfe123db83d790252d6810fa5daf1`.
Their pages are scan evidence, not redistributed fixtures, so their bytes and
license texts are intentionally not copied into this repository.

[ripgrep 15.2.0]: https://github.com/BurntSushi/ripgrep/releases/tag/15.2.0
[rclone v1.75.0]: https://github.com/rclone/rclone/releases/tag/v1.75.0
[fd v10.4.2]: https://github.com/sharkdp/fd/releases/tag/v10.4.2
[zoxide v0.10.0]: https://github.com/ajeetdsouza/zoxide/releases/tag/v0.10.0
[VERIFIED_TOPICS.txt]: VERIFIED_TOPICS.txt
[MIT]: ../LICENSES/RIPGREP-LICENSE-MIT.txt
[Unlicense]: ../LICENSES/RIPGREP-UNLICENSE.txt
[rclone MIT]: ../LICENSES/RCLONE-COPYING.txt
[`RIPGREP-LICENSE-MIT.txt`]: ../LICENSES/RIPGREP-LICENSE-MIT.txt
[`RIPGREP-UNLICENSE.txt`]: ../LICENSES/RIPGREP-UNLICENSE.txt
[`RCLONE-COPYING.txt`]: ../LICENSES/RCLONE-COPYING.txt
