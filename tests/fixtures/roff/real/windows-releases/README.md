# Official Windows release fixtures

These fixtures are exact roff members from official upstream Windows release
archives, losslessly recompressed with zstd. They answer a narrower question
than the distribution corpus: whether manuals shipped to Windows users parse
correctly on ManT's native Windows path. A manual moves to the cross-platform
corpus only after its official Windows and Linux release members have been
proven byte-identical.

The decompressed bytes retain the release archives' line endings. `rg.1` has
mixed CRLF/LF terminators; `rclone.1` and `npm.1` use CRLF throughout;
`scan-build.1` uses LF.

| Fixture | Upstream release | Archive member | Storage | Fixture license | Fixture SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `rg.1.zst` | [ripgrep 15.2.0], `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip` | `ripgrep-15.2.0-x86_64-pc-windows-msvc/doc/rg.1` | Lossless zstd recompression | [MIT] OR [Unlicense] | `d3c33d6550c1ceec1f5dd0ef4d56c0e2e5fc4d64fe6200f04b8527ee1fd5b983` |
| `rclone.1.zst` | [rclone v1.75.0], `rclone-v1.75.0-windows-amd64.zip` | `rclone-v1.75.0-windows-amd64/rclone.1` | Lossless zstd recompression | [rclone MIT] | `4e38c8e1a35e13faafda1d8bfa14b25f792e665476b7405d541e8462e049286e` |
| `npm.1.zst` | [Node.js v24.19.0] (`npm 11.17.0`), `node-v24.19.0-win-x64.zip` | `node-v24.19.0-win-x64/node_modules/npm/man/man1/npm.1` | Lossless zstd recompression | [npm Artistic-2.0] | `e25a8b60365e428c9302f2e036ce3ae0f5d1523d98e680122069c1c440597970` |
| `scan-build.1.zst` | [LLVM 22.1.8], `clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz` | `clang+llvm-22.1.8-x86_64-pc-windows-msvc/share/man/man1/scan-build.1` | Lossless zstd recompression | [LLVM Apache-2.0 exception] | `1e7f95e145309f1624526c86ba676baf052890b446dd376139d86dd95087dd9d` |

The archives were retrieved on 2026-08-11 and have these SHA-256 values:

- `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip`: `71b2fef860abe467217a538ff31de02f5258807c0129f771846f87bd029aafc5`
- `rclone-v1.75.0-windows-amd64.zip`: `203581f0a7baeae873f2347483a798c79e2eaf5c384a4e9d866aa374f1c89ac0`
- `node-v24.19.0-win-x64.zip`: `57f71ab3652e797d84acddc79c81cc9ff1c6ddb2a1974cdb83f00fee9bff4c73`
- `clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz`: `d96c2cc1736f4eb7fa43cb9bbdf56d93551a9ae0a9aadb9c99c3c3b2b712a234`

Before recompression, the archive members have these SHA-256 values:

- `rg.1`: `e83d82c28bb2683cf71500e5fb8d9746a162bb24b08a6396a15ab2ed1d96cb45`
- `rclone.1`: `f35de3b3008f684a7db141a7626db68b08c46e5b3d45c97726bc6d97eebade4e`
- `npm.1`: `34557de7f44cfdaf1bccdc05621bae7108df3bf58c49d24b1688292b82cff158`
- `scan-build.1`: `d0e7de01eb61dd787cf8162f00ab60f68d8b271a084c0f2cbc0652797a01d4b9`

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

The complete npm `LICENSE` and LLVM `LICENSE.TXT` files from the exact release
archives are retained as [`NPM-LICENSE.txt`] and [`LLVM-22.1.8.txt`]. The npm
file's CRLF terminators were normalized to LF without changing its wording.

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

unzip -p node-v24.19.0-win-x64.zip \
  node-v24.19.0-win-x64/node_modules/npm/man/man1/npm.1 > npm.1
zstd -19 -f -o npm.1.zst npm.1

tar -xOf clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz \
  clang+llvm-22.1.8-x86_64-pc-windows-msvc/share/man/man1/scan-build.1 \
  > scan-build.1
zstd -19 -f -o scan-build.1.zst scan-build.1
```

Verify both the decoded member hash and the checked-in zstd hash. Recompression
with another zstd version may produce different container bytes while still
decoding to the required raw hash.

## `mant` parsing verification

On 2026-08-11, 28 official Windows release archives and release packages were
inspected. Ten contained roff; all **338 topic/section requests** completed
ManT's bundled libmandoc path.

No parser crash was observed. As in the Linux distribution corpora, this count
measures parser completion and does not claim perfect structure or typography
for every page. Unlike the broader distribution scans, every page in this
set also had its generated Markdown outline, beginning, and ending reviewed for
empty sections, truncation, control characters, and leaked roff markup. The
291 pages added by the toolchain survey additionally had every heading and
diagnostic reviewed; representative pages from every archive were read in
full.

The scan scope is recorded in [VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt), grouped
by the release archive that ships each page. Archives without roff remain in the
table below as zero-page inspection results; they are not represented as
verified topic requests.

| Release | Language | Windows archive | Topics | Notes |
| --- | --- | --- | ---: | --- |
| [ripgrep 15.2.0] | Rust | `ripgrep-15.2.0-x86_64-pc-windows-msvc.zip` | 1 | Fixed as `rg.1.zst` |
| [fd v10.4.2] | Rust | `fd-v10.4.2-x86_64-pc-windows-msvc.zip` | 1 | scanned |
| [zoxide v0.10.0] | Rust | `zoxide-0.10.0-x86_64-pc-windows-msvc.zip` | 6 | scanned |
| [bat v0.26.1] | Rust | `bat-v0.26.1-x86_64-pc-windows-msvc.zip` | 1 | scanned |
| [hyperfine v1.20.0] | Rust | `hyperfine-v1.20.0-x86_64-pc-windows-msvc.zip` | 1 | scanned |
| [rclone v1.75.0] | Go | `rclone-v1.75.0-windows-amd64.zip` | 1 | Fixed as `rclone.1.zst` |
| [Git LFS v3.7.1] | Go | `git-lfs-windows-amd64-v3.7.1.zip` | 36 | scanned |
| [Node.js v24.19.0] | JavaScript | `node-v24.19.0-win-x64.zip` | 93 | npm has 92 pages; bundled cssesc adds one; `npm(1)` fixed |
| [Git for Windows v2.55.0.windows.3] | C | `mingw-w64-x86_64-git-doc-man-2.55.0.3-1-any.pkg.tar.xz` | 197 | scanned; Windows-specific paths and links |
| [LLVM 22.1.8] | C/C++ | `clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz` | 1 | `scan-build(1)` fixed |
| [Go 1.26.5] | Go | `go1.26.5.windows-amd64.zip` | 0 | no roff in archive |
| [Ninja 1.13.2] | C++ | `ninja-win.zip` | 0 | no roff in archive |
| [Python 3.14.6] | C/Python | `python-3.14.6-embed-amd64.zip` | 0 | no roff in archive |
| [Zig 0.16.0] | Zig | `zig-x86_64-windows-0.16.0.zip` | 0 | no roff in archive |
| [eza v0.23.5] | Rust | `eza.exe_x86_64-pc-windows-gnu.zip` | 0 | no roff in archive |
| [starship v1.26.0] | Rust | `starship-x86_64-pc-windows-msvc.zip` | 0 | no roff in archive |
| [bottom 0.14.7] | Rust | `bottom_x86_64-pc-windows-msvc.zip` | 0 | no roff in archive |
| [delta 0.19.2] | Rust | `delta-0.19.2-x86_64-pc-windows-msvc.zip` | 0 | no roff in archive |
| [dust v1.2.4] | Rust | `dust-v1.2.4-x86_64-pc-windows-msvc.zip` | 0 | no roff in archive |
| [GitHub CLI v2.97.0] | Go | `gh_2.97.0_windows_amd64.zip` | 0 | no roff in archive |
| [age v1.3.1] | Go | `age-v1.3.1-windows-amd64.zip` | 0 | no roff in archive |
| [restic v0.19.1] | Go | `restic_0.19.1_windows_amd64.zip` | 0 | no roff in archive |
| [fzf v0.74.2] | Go | `fzf-0.74.2-windows_amd64.zip` | 0 | no roff in archive |
| [lazygit v0.64.0] | Go | `lazygit_0.64.0_windows_x86_64.zip` | 0 | no roff in archive |
| [Syncthing v2.1.3] | Go | `syncthing-windows-amd64-v2.1.3.zip` | 0 | no roff in archive |
| [yq v4.53.3] | Go | `yq_windows_amd64.zip` | 0 | no roff in archive |
| [Hugo v0.164.0] | Go | `hugo_0.164.0_windows-amd64.zip` | 0 | no roff in archive |
| [Caddy v2.11.4] | Go | `caddy_2.11.4_windows_amd64.zip` | 0 | no roff in archive |

The fd archive SHA-256 is
`b2816e506390a89941c63c9187d58a3cc10e9a55f2ef0685f9ea0eccaf7c98c8`; the
zoxide archive SHA-256 is
`f465ae548f8754c8e7edbc60b45fbf58c92bfe123db83d790252d6810fa5daf1`.
The additionally scanned archives have these SHA-256 values:

- `bat-v0.26.1-x86_64-pc-windows-msvc.zip`: `0f729b4b6f5f28d395c641eacc2e9ff68d0096b85aa0eec344aa62425144b69b`
- `hyperfine-v1.20.0-x86_64-pc-windows-msvc.zip`: `2508c549b049b1d4342d08edc1cb42bfac169082b6e3069431b5bab9822dbb32`
- `git-lfs-windows-amd64-v3.7.1.zip`: `8683cdc3d6c029b49393dcebbaa6265bd6efd9abdcf837be855b4cd42e5e80b6`
- `node-v24.19.0-win-x64.zip`: `57f71ab3652e797d84acddc79c81cc9ff1c6ddb2a1974cdb83f00fee9bff4c73`
- `mingw-w64-x86_64-git-doc-man-2.55.0.3-1-any.pkg.tar.xz`: `631d44bd9f469a705da8f7b4f0bfff39fa33274ff497c439eb9d21d77d11ba92`
- `clang+llvm-22.1.8-x86_64-pc-windows-msvc.tar.xz`: `d96c2cc1736f4eb7fa43cb9bbdf56d93551a9ae0a9aadb9c99c3c3b2b712a234`
- `go1.26.5.windows-amd64.zip`: `97e6b2a833b6d89f9ff17d25419ac0a7e3b482a044e9ab18cdef834bd834fd38`
- `ninja-win.zip`: `07fc8261b42b20e71d1720b39068c2e14ffcee6396b76fb7a795fb460b78dc65`
- `python-3.14.6-embed-amd64.zip`: `df901e84a896ff1ee720ad03377e0c8d8c2244fda79808aeeaff6316df1cb75c`
- `zig-x86_64-windows-0.16.0.zip`: `68659eb5f1e4eb1437a722f1dd889c5a322c9954607f5edcf337bc3684a75a7e`

Pages not named in the fixed-fixture table are scan evidence rather than
redistributed fixtures, so their bytes and license texts are intentionally not
copied into this repository. The 38 pages added by the earlier Rust/Go CLI scan
produced 237 section nodes and 3,035 lines of Markdown. Full
representative-content review covered bat, hyperfine, and Git LFS's main,
completion, migrate, config(5), and faq(7) pages.

Cargo 1.97.1, rustc 1.97.1, and CMake 4.4.2 are intentionally not counted in
this Windows table: all 67 of their Windows manuals were proven byte-identical
to the corresponding official Linux release members and are recorded in the
separate [cross-platform release corpus]. Node/npm is content-identical to its
Linux counterpart only after removing CR characters, so its raw Windows pages
remain here.

Observed upstream-generator behaviour:

- Git LFS's Asciidoctor template requests the denied external `www.tmac` file
  and an unknown colour-only `LINKSTYLE` macro on every page. The local URL
  macro and document bodies remain intact.
- `git-lfs-push(1)` omits breaks between its final three synopsis forms, so the
  joined rendering follows the release bytes.
- Hyperfine has one unmatched final `.RE`; its examples and `AUTHOR` section
  remain complete.
- npm's generator escaped redundant `\fB`/`\fR` requests inside already-bold
  labels. ManT now removes only those same-font duplicates; literal formatter
  examples in regular text remain intact. All 93 Node/npm bundle pages were
  reviewed after the fix.
- Git for Windows includes MinGW-specific paths and generated links, so its 197
  pages are Windows evidence rather than byte-identical cross-platform input.
- LLVM 22.1.8 ships one manual, `scan-build(1)`. Its embedded `clang 20` and
  August 2024 metadata are stale upstream release bytes and are intentionally
  retained.
- The fixed ripgrep page yields 14 top-level sections and 104 semantic option
  entries. The larger Pandoc-generated rclone page yields 191 top-level and
  3,430 total section nodes and reports diagnostics for GNU font extensions
  such as `\f[V]`; `VERIFIED_TOPICS.txt` therefore makes no zero-warning claim.

[ripgrep 15.2.0]: https://github.com/BurntSushi/ripgrep/releases/tag/15.2.0
[rclone v1.75.0]: https://github.com/rclone/rclone/releases/tag/v1.75.0
[fd v10.4.2]: https://github.com/sharkdp/fd/releases/tag/v10.4.2
[zoxide v0.10.0]: https://github.com/ajeetdsouza/zoxide/releases/tag/v0.10.0
[bat v0.26.1]: https://github.com/sharkdp/bat/releases/tag/v0.26.1
[hyperfine v1.20.0]: https://github.com/sharkdp/hyperfine/releases/tag/v1.20.0
[Git LFS v3.7.1]: https://github.com/git-lfs/git-lfs/releases/tag/v3.7.1
[Node.js v24.19.0]: https://nodejs.org/en/blog/release/v24.19.0
[Git for Windows v2.55.0.windows.3]: https://github.com/git-for-windows/git/releases/tag/v2.55.0.windows.3
[LLVM 22.1.8]: https://github.com/llvm/llvm-project/releases/tag/llvmorg-22.1.8
[Go 1.26.5]: https://go.dev/dl/#go1.26.5
[Ninja 1.13.2]: https://github.com/ninja-build/ninja/releases/tag/v1.13.2
[Python 3.14.6]: https://www.python.org/downloads/release/python-3146/
[Zig 0.16.0]: https://ziglang.org/download/0.16.0/
[eza v0.23.5]: https://github.com/eza-community/eza/releases/tag/v0.23.5
[starship v1.26.0]: https://github.com/starship/starship/releases/tag/v1.26.0
[bottom 0.14.7]: https://github.com/ClementTsang/bottom/releases/tag/0.14.7
[delta 0.19.2]: https://github.com/dandavison/delta/releases/tag/0.19.2
[dust v1.2.4]: https://github.com/bootandy/dust/releases/tag/v1.2.4
[GitHub CLI v2.97.0]: https://github.com/cli/cli/releases/tag/v2.97.0
[age v1.3.1]: https://github.com/FiloSottile/age/releases/tag/v1.3.1
[restic v0.19.1]: https://github.com/restic/restic/releases/tag/v0.19.1
[fzf v0.74.2]: https://github.com/junegunn/fzf/releases/tag/v0.74.2
[lazygit v0.64.0]: https://github.com/jesseduffield/lazygit/releases/tag/v0.64.0
[Syncthing v2.1.3]: https://github.com/syncthing/syncthing/releases/tag/v2.1.3
[yq v4.53.3]: https://github.com/mikefarah/yq/releases/tag/v4.53.3
[Hugo v0.164.0]: https://github.com/gohugoio/hugo/releases/tag/v0.164.0
[Caddy v2.11.4]: https://github.com/caddyserver/caddy/releases/tag/v2.11.4
[MIT]: ../LICENSES/RIPGREP-LICENSE-MIT.txt
[Unlicense]: ../LICENSES/RIPGREP-UNLICENSE.txt
[rclone MIT]: ../LICENSES/RCLONE-COPYING.txt
[npm Artistic-2.0]: ../LICENSES/NPM-LICENSE.txt
[LLVM Apache-2.0 exception]: ../LICENSES/LLVM-22.1.8.txt
[`RIPGREP-LICENSE-MIT.txt`]: ../LICENSES/RIPGREP-LICENSE-MIT.txt
[`RIPGREP-UNLICENSE.txt`]: ../LICENSES/RIPGREP-UNLICENSE.txt
[`RCLONE-COPYING.txt`]: ../LICENSES/RCLONE-COPYING.txt
[`NPM-LICENSE.txt`]: ../LICENSES/NPM-LICENSE.txt
[`LLVM-22.1.8.txt`]: ../LICENSES/LLVM-22.1.8.txt
[cross-platform release corpus]: ../cross-platform-releases/README.md
