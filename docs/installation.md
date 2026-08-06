# Installation

The one-line installers are the recommended way to install the latest ManT
release. They also register the bundled `mant.md` document, making the complete
manual available through `mant mant`, structured queries, and MCP discovery.

## Recommended installers

### Unix (Linux or macOS)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1 | iex
```

Both scripts select the latest public GitHub release by default. Running the
same command again updates an older installation or reports that the installed
version is current. Linux and Windows downloads are verified against that
release's `SHA256SUMS` manifest before installation.

## Choose an installation method

| Method | Linux x64/arm64 | macOS | Windows x64 | Registers `mant.md` |
| --- | --- | --- | --- | --- |
| One-line installer | Prebuilt archive | Cargo source build | Prebuilt archive | Yes |
| `cargo-binstall` | Prebuilt archive | Cargo fallback | Prebuilt archive | No |
| `cargo install` | Source build | Source build | Source build | No |
| Manual archive | Prebuilt archive | Not published | Prebuilt archive | Optional |
| Repository checkout | Source build | Source build | Source build | No |

Linux and macOS builds parse Markdown and native man/mdoc documents. Windows
provides the same Markdown, TUI, structured output, tldr, and MCP interfaces,
but does not parse Unix man/roff sources.

## Installer behavior and options

On Linux, the Unix installer selects the x64 or arm64 archive, installs the
executable to `~/.local/bin`, and installs the manual below
`${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents`. Ensure
`~/.local/bin` is on `PATH`.

Public macOS archives remain disabled until they can be Developer ID-signed
and notarized. The Unix installer therefore builds the selected release from
crates.io on macOS, installs it to `~/.local/bin`, and registers its manual
under `~/Library/Application Support/ManT/documents`.

On Windows, the PowerShell installer uses the x64 ZIP, installs `mant.exe`
below `%LOCALAPPDATA%\Programs\ManT\bin`, adds that directory to the user
`PATH`, and registers the manual below `%APPDATA%\ManT\documents`.

Set `MANT_VERSION` to a release such as `0.5.0` to install that version instead
of the latest. `MANT_INSTALL_DIR` and `MANT_DATA_DIR` override the executable
and document destinations.

The scripts also accept command-line options. Pass them to the receiving shell
on Unix:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh -s -- --version 0.5.0
```

Create and invoke a script block when passing PowerShell parameters:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1))) -Version 0.5.0
```

Use `--help` or `-Help` to list every option. Notable controls include
`--no-manual`/`-NoManual`, `--force`/`-Force`, and the Windows-only
`-NoModifyPath`.

## Update and uninstall

The recommended installation command is also the update command. It compares
the selected release with the installed `mant --version`, avoids downloading
an already-current installation, and repairs a missing bundled manual. The
explicit `--update` and `-Update` options are aliases for callers that want to
state their intent.

The installer writes a private receipt containing only the owned binary,
manual, version, and installation directories. Windows also records whether
the installer added its directory to user `PATH`. Uninstall removes only those
exact files, never recursively deletes their parent directories, and removes
the Windows PATH entry only when the receipt says the installer added it.

Uninstall on Unix:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.sh | sh -s -- --uninstall
```

Uninstall on Windows:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/BryanHeBY/ManT/main/scripts/install.ps1))) -Uninstall
```

The receipt lives below `${XDG_STATE_HOME:-$HOME/.local/state}/mant` on Linux,
Application Support on macOS, and `%LOCALAPPDATA%\ManT` on Windows. An older
one-line installation without a receipt can be adopted by running the current
installer once before uninstalling it.

## cargo-binstall

[`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) uses ManT's
native release archives when one matches the current platform:

```sh
cargo binstall mant
mant git
```

Targets without a matching archive fall back to a Cargo source build.
`cargo-binstall` installs the executable but does not register the optional
`mant.md` manual.

## Cargo source installation

Compile and install the latest published crate explicitly:

```sh
cargo install mant --locked
mant git
```

This requires Rust 1.88+. Linux additionally requires a C compiler and zlib
development headers; macOS requires Clang and zlib. Windows builds are pure
Rust. On Unix, neither a `man` nor a `mandoc` executable is required at
runtime.

## Manual release archives

### Linux

Download the archive for your architecture from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), then
install the executable and its bundled manual:

```sh
tar -xzf mant-<version>-linux-<arch>.tar.gz
cd mant-<version>-linux-<arch>
install -Dm755 mant ~/.local/bin/mant
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install -Dm644 mant.md "$data_home/mant/documents/mant.md"
mant mant
```

For a system-wide installation, use `/usr/local/bin/mant` and
`/usr/local/share/mant/documents/mant.md` instead. User documents take
precedence over system documents. The archive also contains the project
README, the Apache-2.0 license, and the bundled mandoc license.

### Windows

Download `mant-<version>-windows-x64.zip` from the
[latest release](https://github.com/BryanHeBY/ManT/releases/latest), extract
`mant.exe` into a directory on `PATH`, and optionally register the bundled
manual:

```powershell
$documents = Join-Path $env:APPDATA "ManT\documents"
New-Item $documents -ItemType Directory -Force | Out-Null
Copy-Item .\mant.md (Join-Path $documents "mant.md")
mant mant
```

## Build from a repository checkout

```sh
git clone https://github.com/BryanHeBY/ManT.git
cd ManT
cargo build --release --locked -p mant
./target/release/mant git
```

See the [development guide](development.md) for the complete repository check
and fixture requirements.
