# Fixed real-man roff fixtures

These compressed files are fixed snapshots of manual pages shipped by the
official Linux packages listed below. They are parsed directly through ManT's
bundled libmandoc; tests never consult the host manual database for them.

The snapshots replace the former renderer-specific HTML fixtures. Keeping fixed
compressed roff sources makes parser regressions reproducible across Linux and
macOS while retaining all notices embedded by the upstream documentation.

`fedora44/` contains a second fixed set derived from Fedora Linux 44 packages.
Its roff bytes are preserved exactly and losslessly recompressed with zstd,
covering the in-process zstd source path as well as current GCC, Clang, Git,
and GNU tar generator output. See [`fedora44/README.md`](fedora44/README.md)
for the exact RPM and source-RPM provenance, hashes, and license mapping.

## Provenance and licensing

| Fixture | Upstream and Arch package | Package path | Fixture license | SHA-256 |
| --- | --- | --- | --- | --- |
| `ls.1.gz` | [GNU coreutils], [Arch `coreutils` 9.11-2] | `usr/share/man/man1/ls.1.gz` | [GPL-3.0-or-later] | `091e614c945887862980212abe697c63b946fbb4d189c741ad47c5dd71bd4ea0` |
| `git.1.gz` | [Git], [Arch `git` 2.55.0-1] | `usr/share/man/man1/git.1.gz` | [GPL-2.0-only] | `8b58cbf77d1eb0ca9efcea2a98790574dcf3c2f76d02ce08531af1e931a926ed` |
| `gcc.1.gz` | [GCC], [Arch `gcc` 16.1.1+r346+g4e03491b401d-4] | `usr/share/man/man1/gcc.1.gz` | [GFDL-1.3-invariants-or-later] | `8a0bbfaaa5b05a8fcefc6d4741530d09abcfd95b26f8947e9aecbce68cb75b23` |
| `clang.1.gz` | [LLVM Clang], [Arch `clang` 22.1.8-1] | `usr/share/man/man1/clang.1.gz` | [Apache-2.0 WITH LLVM-exception] | `313398b1f95b070d7a807ea8cc2d28403b0e25159960b9fa9ce90d820bff5bed` |
| `tar.1.gz` | [GNU tar], [Arch `tar` 1.35-2] | `usr/share/man/man1/tar.1.gz` | [GPL-3.0-or-later] | `dfeee239e4bbed1d271c0902c0fce79e5844c4d4778deae3e8d9c9995341c726` |

The GCC page's own notice names the invariant sections "GNU General Public
License" and "Funding Free Software" and specifies its front- and back-cover
texts. Those page-specific conditions remain embedded verbatim in `gcc.1.gz`;
the local GFDL file supplies the complete license text it references.

`LLVM.txt` is the complete license file shipped by the matching Arch Clang
package. It contains the Apache License 2.0, the LLVM exception, and LLVM's
legacy license notice rather than silently narrowing the upstream terms.

These licenses govern only their respective third-party fixtures. ManT's own
source remains under the repository-level Apache-2.0 license.

## Updating a fixture

Download the exact package from the Arch Linux Archive and extract its already
compressed man page without recompressing it. For example:

```sh
curl -LO https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
bsdtar -xOf coreutils-9.11-2-x86_64.pkg.tar.zst \
  usr/share/man/man1/ls.1.gz > ls.1.gz
sha256sum ls.1.gz
```

When replacing a fixture, update the archive URL, acquisition date, version,
hash, applicable license files, and topology assertions in the same commit.

[GNU coreutils]: https://www.gnu.org/software/coreutils/
[Git]: https://git-scm.com/
[GCC]: https://gcc.gnu.org/
[LLVM Clang]: https://clang.llvm.org/
[GNU tar]: https://www.gnu.org/software/tar/
[Arch `coreutils` 9.11-2]: https://archive.archlinux.org/packages/c/coreutils/coreutils-9.11-2-x86_64.pkg.tar.zst
[Arch `git` 2.55.0-1]: https://archive.archlinux.org/packages/g/git/git-2.55.0-1-x86_64.pkg.tar.zst
[Arch `gcc` 16.1.1+r346+g4e03491b401d-4]: https://archive.archlinux.org/packages/g/gcc/gcc-16.1.1%2Br346%2Bg4e03491b401d-4-x86_64.pkg.tar.zst
[Arch `clang` 22.1.8-1]: https://archive.archlinux.org/packages/c/clang/clang-22.1.8-1-x86_64.pkg.tar.zst
[Arch `tar` 1.35-2]: https://archive.archlinux.org/packages/t/tar/tar-1.35-2-x86_64.pkg.tar.zst
[GPL-2.0-only]: LICENSES/GPL-2.0-only.txt
[GPL-3.0-or-later]: LICENSES/GPL-3.0-or-later.txt
[GFDL-1.3-invariants-or-later]: LICENSES/GFDL-1.3-invariants-or-later.txt
[Apache-2.0 WITH LLVM-exception]: LICENSES/LLVM.txt
