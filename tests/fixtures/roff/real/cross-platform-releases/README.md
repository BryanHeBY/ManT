# Byte-identical cross-platform release fixtures

This corpus contains manuals whose decompressed bytes are identical in the
official x86_64 Windows and Linux release archives. It is separate from the
Windows corpus so that platform-neutral toolchain evidence is not presented as
a Windows-specific parser case. Content-equivalent files with platform line
ending changes do not qualify.

| Fixture | Upstream release | Windows / Linux archive members | Fixture license | Raw SHA-256 | Fixture SHA-256 |
| --- | --- | --- | --- | --- | --- |
| `cargo.1.zst` | [Rust 1.97.1] (`cargo 1.97.1`) | `cargo-1.97.1-x86_64-pc-windows-msvc/cargo/share/man/man1/cargo.1` / `cargo-1.97.1-x86_64-unknown-linux-gnu/cargo/share/man/man1/cargo.1` | [Cargo Apache-2.0] OR [Cargo MIT] | `c483b800c9aceb307fb15ea4138783838cb379b6f4e306a8dabe84af8fd4b01d` | `cf99cb41ada0df8c560806c4a3a111dd0640801147ec434e53d31a0b75637e09` |
| `rustc.1.zst` | [Rust 1.97.1] | `rustc-1.97.1-x86_64-pc-windows-msvc/rustc/share/man/man1/rustc.1` / `rustc-1.97.1-x86_64-unknown-linux-gnu/rustc/share/man/man1/rustc.1` | [Rust Apache-2.0] OR [Rust MIT] | `03a056a5b5041454e837f12027e3002ceee1e452530d17a5c55edf14183b9afb` | `4b75c0373306c24c1b6160a6fb4dcc8deb6a2c57413be694f0d6d510f0c0de1f` |
| `cmake-toolchains.7.zst` | [CMake 4.4.2] | `cmake-4.4.2-windows-x86_64/man/man7/cmake-toolchains.7` / `cmake-4.4.2-linux-x86_64/share/man/man7/cmake-toolchains.7` | [CMake BSD-3-Clause] | `4f5e1946154896899d6471c4e5b0bf902394429641231074dcacc71bbc5dd20b` | `d8d0fb4d92127a154c2ba5704a550bbeb832eeaf6476059a9354a5e509c1611f` |

All 37 Cargo pages, both rustc pages, and all 28 CMake pages were compared,
not only the three fixed fixtures. Every matching Windows/Linux member pair
was byte-identical, for a total of **67 pages**.

The release archives were retrieved on 2026-08-11 and have these SHA-256
values:

- `cargo-1.97.1-x86_64-pc-windows-msvc.tar.xz`: `1180ac0cd30ee98af682528c10505f5cba118f122aec9b7ca18ae605b1db38a0`
- `cargo-1.97.1-x86_64-unknown-linux-gnu.tar.xz`: `e1be5f5ff7f7f80ca506fb65770b759edbdc6d303781ed71c5de8ec8a8394779`
- `rustc-1.97.1-x86_64-pc-windows-msvc.tar.xz`: `0119e2788f3391a891b2e0fe611e82b433670eeae76c45995b081d0ac7715c6d`
- `rustc-1.97.1-x86_64-unknown-linux-gnu.tar.xz`: `9819d0a32d56bd339585319c80260e332779f5541fd66838ab7e016d6c814819`
- `cmake-4.4.2-windows-x86_64.zip`: `e8139d85b3813bc38833142ae1940472e9a587e9b5d2718ac1804c60f4e57a64`
- `cmake-4.4.2-linux-x86_64.tar.gz`: `3ada9a3f5d8a85413579bdd0ea6aa8e8da86efdd6d15c91a1afa517f2021956c`

The fixed members were losslessly recompressed with Zstandard CLI 1.5.7 and
`zstd -19`. Decoding reproduces the raw hashes in the table. Another zstd
version may produce different container bytes without changing the fixture.

## License mapping

The complete Cargo and rustc Apache-2.0 and MIT license files from their
respective release archives are retained separately because their upstream
texts are not byte-identical. CMake's complete `Copyright.txt` is retained as
[`CMAKE-LICENSE.rst`]; its CRLF line endings were normalized to LF without
changing the wording.

These notices apply only to the third-party fixture bytes. They do not change
ManT's repository-level Apache-2.0 license.

## Reproducing the fixtures

Extract the same member from either member of each verified platform pair and
recompress it without newline conversion:

```sh
tar -xOf cargo-1.97.1-x86_64-pc-windows-msvc.tar.xz \
  cargo-1.97.1-x86_64-pc-windows-msvc/cargo/share/man/man1/cargo.1 > cargo.1
zstd -19 -f -o cargo.1.zst cargo.1

tar -xOf rustc-1.97.1-x86_64-pc-windows-msvc.tar.xz \
  rustc-1.97.1-x86_64-pc-windows-msvc/rustc/share/man/man1/rustc.1 > rustc.1
zstd -19 -f -o rustc.1.zst rustc.1

unzip -p cmake-4.4.2-windows-x86_64.zip \
  cmake-4.4.2-windows-x86_64/man/man7/cmake-toolchains.7 > cmake-toolchains.7
zstd -19 -f -o cmake-toolchains.7.zst cmake-toolchains.7
```

## `mant` parsing verification

On 2026-08-11, all **67 topic/section requests** completed ManT's bundled
libmandoc path with no crash or parser diagnostic. Every generated Markdown
document was reviewed through a complete structural summary covering line
count, all headings, first and last sections, diagnostics, empty output,
control characters, and leaked line-start roff macros.

Full representative-content review additionally covered Cargo's main and
subcommand pages, `rustc(1)`, `rustdoc(1)`, CMake's main, toolchains, and server
references. The fixed tests pin command/option outlines, compiler and
cross-compilation content, source metadata, and spacing/markup invariants.

The exact scan scope is recorded in
[VERIFIED_TOPICS.txt](VERIFIED_TOPICS.txt).

[Rust 1.97.1]: https://static.rust-lang.org/dist/2026-07-16/
[CMake 4.4.2]: https://github.com/Kitware/CMake/releases/tag/v4.4.2
[Cargo Apache-2.0]: ../LICENSES/CARGO-LICENSE-APACHE.txt
[Cargo MIT]: ../LICENSES/CARGO-LICENSE-MIT.txt
[Rust Apache-2.0]: ../LICENSES/RUST-LICENSE-APACHE.txt
[Rust MIT]: ../LICENSES/RUST-LICENSE-MIT.txt
[CMake BSD-3-Clause]: ../LICENSES/CMAKE-LICENSE.rst
[`CMAKE-LICENSE.rst`]: ../LICENSES/CMAKE-LICENSE.rst
