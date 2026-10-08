# Installation

The one-line installers are the recommended way to install the latest ManT
release. They also register the bundled command, protocol, IR, Markdown, and
roff manuals, making them available through ordinary discovery, structured
queries, and MCP.

## Recommended installers

### Unix (Linux with glibc, or macOS)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1 | iex
```

Both scripts select the latest public GitHub release by default. Running the
same command again updates an older installation or reports that the installed
version is current. Every downloaded archive is verified against that
release's `SHA256SUMS` manifest before installation. If an authenticated
GitHub CLI (`gh`) is available, the scripts also require its GitHub provenance
attestation to verify before installing the archive.

## Choose an installation method

| Method | Linux glibc x64/arm64 | macOS | Windows x64 | Registers manuals |
| --- | --- | --- | --- | --- |
| One-line installer | Prebuilt archive | Cargo source build | Prebuilt archive | Yes |
| `cargo-binstall` | Prebuilt archive | Cargo fallback | Prebuilt archive | No |
| `cargo install` | Source build | Source build | Source build | No |
| Manual archive | Bundled with binary | Manuals only | Bundled with binary | Optional |
| Repository checkout | Source build | Source build | Source build | No |

Linux with glibc, macOS, and Windows parse Markdown and native man/mdoc
documents and provide the same TUI, structured output, tldr, and MCP
interfaces. Linux systems using musl, including Alpine Linux, are not currently
supported; the installer rejects them before downloading an archive.

## Installer behavior and options

On Linux, the Unix installer selects the x64 or arm64 archive, installs the
executable to `~/.local/bin`, and installs the manuals below
`${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents`. Ensure
`~/.local/bin` is on `PATH`.

Public macOS archives remain disabled until they can be Developer ID-signed
and notarized. The Unix installer therefore builds the selected release from
crates.io on macOS, installs it to `~/.local/bin`, and registers its manuals
from the checksummed, attested `mant-<version>-manuals.tar.gz` release asset
under the configured ManT data root's `documents/` directory. This build requires Rust
1.88 or newer, Clang, and zlib to be available before running the installer.
Releases before 0.7.0 did not publish that asset, so selecting one of them on
macOS installs only its binary instead of downloading unverified manual files.

On Windows, the PowerShell installer uses the x64 ZIP, installs `mant.exe`
below `~/.local/bin`, adds that directory to the user `PATH`, and registers
the manuals below `~/.local/share/mant/documents`, like Linux and macOS.
All three installers ask the verified target binary to resolve `mant.toml`,
`MANT_*_HOME`, and XDG overrides; see [configuration](configuration.md).

Set `MANT_VERSION` to a release such as `MAJOR.MINOR.PATCH` to install that
version instead of the latest. `MANT_INSTALL_DIR` and `MANT_DATA_DIR` override
the executable and document destinations. `MANT_DATA_DIR` is the directory
that directly receives the bundled `.md` files, not the parent ManT data root.

The scripts also accept command-line options. Pass them to the receiving shell
on Unix:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh -s -- --version MAJOR.MINOR.PATCH
```

Create and invoke a script block when passing PowerShell parameters:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1))) -Version MAJOR.MINOR.PATCH
```

Use `--help` or `-Help` to list every option. Notable controls include
`--no-manual`/`-NoManual`, `--force`/`-Force`, and the Windows-only
`-NoModifyPath`.

## Update and uninstall

The recommended installation command is also the update command. It compares
the selected release with the installed `mant --version`, avoids downloading
an already-current installation, and repairs any missing bundled manual. The
explicit `--update` and `-Update` options are aliases for callers that want to
state their intent.

The installer writes a private receipt containing only the owned binary,
manual files, version, and installation directories. Windows also records whether
the installer added its directory to user `PATH`. Uninstall removes only those
exact files, never recursively deletes their parent directories, and removes
an installer-owned dedicated Windows PATH entry only when the receipt says
the installer added it. Shared `~/.local/bin` stays on PATH during uninstall.

Uninstall on Unix:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh -s -- --uninstall
```

Uninstall on Windows:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1))) -Uninstall
```

The receipt lives below `${XDG_STATE_HOME:-$HOME/.local/state}/mant` on all
three platforms. Old macOS and Windows receipts remain readable for upgrades
and uninstall. The first upgrade to 0.12+ copies known old default configuration
and storage into the new layout, retaining originals. Existing target
configuration wins; conflicting data, links, active source updates, or an
exceeded migration budget stop installation before replacing the binary.
Subsequent upgrades do not re-import the retained old data. Explicit custom
binary/manual destinations remain respected. An older installation without a receipt can be adopted by
running the current installer once before uninstalling it.

The state file is one minimal installation receipt, not a configuration file or
cache: Unix writes `install-receipt` (tab-separated text), and Windows writes
`install-receipt.json`. It also retains migration completion and custom/runtime
document binding. Keep the active receipt for safe uninstall and future upgrades;
there is no separate migration log or cleanup-status file. Successful installs
and already-current checks print its location and optional cleanup guidance.

Original configuration/data, old caches, old-location receipts and binaries
left at a different installation location are not automatically removed. After
`mant --doctor`, checking document discovery and confirming that no configuration,
source or environment override still references them, you may manually remove
obsolete ManT-specific files. Never delete shared/active directories or external
tldr caches. Directory overrides are not rewritten; review outdated custom
ManT overrides and manually added PATH entries yourself. Do not unset `HOME`,
`USERPROFILE`, `APPDATA` or shared XDG directory variables to clean old ManT
storage. Windows may still adjust receipt-owned PATH entries as documented above.
Cleanup is optional.

## cargo-binstall

[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) uses ManT's
native release archives when one matches the current platform:

```sh
cargo binstall mant
mant git
```

Targets without a matching archive fall back to a Cargo source build.
`cargo-binstall` installs the executable but does not register the optional
bundled manuals.

## Cargo source installation

Compile and install the latest published crate explicitly:

```sh
cargo install mant --locked
mant git
```

This requires Rust 1.88+. Linux builds require glibc, a C compiler, and zlib
development headers; macOS requires Clang and zlib. Windows requires the MSVC
C toolchain but no system zlib. Neither a `man` nor a `mandoc` executable is
required at runtime.

## Manual release archives

Every release from 0.7.0 onward also publishes
`mant-<version>-manuals.tar.gz`, a portable archive containing the complete
manual set, its manifest, and applicable license notices. It is useful on
platforms installed from crates.io and is the source used by the macOS
one-line installer.

### Linux with glibc

Download the archive for your architecture from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), then
install the executable and its bundled manuals:

```sh
tar -xzf mant-<version>-linux-<arch>.tar.gz
cd mant-<version>-linux-<arch>
install -Dm755 mant ~/.local/bin/mant
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install -d "$data_home/mant/documents"
install -m644 manuals/*.md "$data_home/mant/documents/"
mant mant
```

For a system-wide executable installation, use `/usr/local/bin/mant`; reusable
Markdown still belongs in each user's data directory. The archive also
contains the project README, the Apache-2.0 license, and a `LICENSES/` bundle
with the generated Rust dependency report, product third-party notice,
libmandoc notices, CC BY 4.0 text, upstream inventory, and complete reusable
terms.

### Windows

Download `mant-<version>-windows-x64.zip` from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), extract
`mant.exe` into a directory on `PATH`, and optionally register the bundled
manuals:

```powershell
$mantHome = if ($env:HOME) { $env:HOME } else { $env:USERPROFILE }
$documents = Join-Path $mantHome ".local\share\mant\documents"
New-Item $documents -ItemType Directory -Force | Out-Null
Copy-Item .\manuals\*.md $documents
mant mant
```

The ZIP has the same documentation and complete `LICENSES\` bundle as the
Linux archives, including `RUST_DEPENDENCIES.html`,
`PRODUCT_THIRD_PARTY_NOTICES.md`, `CC-BY-4.0.txt`, and the libmandoc notices.

## Build from a repository checkout

```sh
git clone https://github.com/BryanHeBY/ManT.git
cd ManT
cargo build --release --locked -p mant
./target/release/mant git
```

See the [development guide](development.md) for the complete repository check
and fixture requirements.
