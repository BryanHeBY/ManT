#!/bin/sh
# Install, update, or uninstall ManT for one Unix user.

set -eu

REPOSITORY=BryanHeBY/ManT
GITHUB_URL="https://github.com/$REPOSITORY"
RECEIPT_SCHEMA=mant.install/v1
BUNDLED_MANUALS="mant.md mant-ir.md mant-markdown.md mant-protocol.md mant-roff.md"

fail() {
  printf 'mant installer: %s\n' "$1" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

require_glibc() {
  if command -v getconf >/dev/null 2>&1 \
    && getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
    return
  fi

  libc_version=
  if command -v ldd >/dev/null 2>&1; then
    libc_version=$(ldd --version 2>&1 || :)
  fi
  case $libc_version in
    *GLIBC*|*"GNU libc"*|*"GNU C Library"*) return ;;
  esac

  fail "public Linux archives require glibc; musl and other libc implementations are not supported"
}

usage() {
  cat <<'EOF'
Install, update, or uninstall ManT.

Usage:
  install.sh [options]

Options:
  --update                 Explicit alias for the default install/update action
  --uninstall              Remove files owned by the one-line installer
  --version VERSION        Install a specific release instead of latest
  --install-dir DIRECTORY  Override the executable directory
  --data-dir DIRECTORY     Override the registered-document directory
  --no-manual              Do not install the bundled ManT manuals
  --force                  Reinstall even when the selected version is current
  -h, --help               Show this help

MANT_VERSION, MANT_INSTALL_DIR, and MANT_DATA_DIR provide the same overrides.
EOF
}

uninstall=false
update_requested=false
force=false
install_manual=true
requested_version=${MANT_VERSION:-}
install_dir_override=${MANT_INSTALL_DIR:-}
data_dir_override=${MANT_DATA_DIR:-}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --update)
      update_requested=true
      ;;
    --uninstall)
      uninstall=true
      ;;
    --version)
      [ "$#" -ge 2 ] || fail "--version requires a value"
      requested_version=$2
      shift
      ;;
    --version=*) requested_version=${1#*=} ;;
    --install-dir)
      [ "$#" -ge 2 ] || fail "--install-dir requires a value"
      install_dir_override=$2
      shift
      ;;
    --install-dir=*) install_dir_override=${1#*=} ;;
    --data-dir)
      [ "$#" -ge 2 ] || fail "--data-dir requires a value"
      data_dir_override=$2
      shift
      ;;
    --data-dir=*) data_dir_override=${1#*=} ;;
    --no-manual) install_manual=false ;;
    --force) force=true ;;
    -h|--help)
      usage
      exit 0
      ;;
    --) shift; break ;;
    *) fail "unknown option '$1'; use --help for usage" ;;
  esac
  shift
done
[ "$#" -eq 0 ] || fail "unexpected positional arguments; use --help for usage"
[ "$uninstall" = false ] || [ "$update_requested" = false ] \
  || fail "--uninstall cannot be combined with --update"

require awk
require grep
require uname
[ -n "${HOME:-}" ] || fail "HOME is required"

case $(uname -s) in
  Linux)
    host=linux
    default_install_dir="$HOME/.local/bin"
    default_data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/mant/documents"
    state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/mant"
    ;;
  Darwin)
    host=macos
    default_install_dir="$HOME/.local/bin"
    default_data_dir="$HOME/Library/Application Support/ManT/documents"
    state_dir="$HOME/Library/Application Support/ManT"
    ;;
  *) fail "operating system '$(uname -s)' is not supported by this installer" ;;
esac

receipt="$state_dir/install-receipt"

receipt_value() {
  awk -v key="$1" 'index($0, key "\t") == 1 {
    print substr($0, length(key) + 2)
    exit
  }' "$receipt"
}

receipt_values() {
  awk -v key="$1" 'index($0, key "\t") == 1 {
    print substr($0, length(key) + 2)
  }' "$receipt"
}

validate_path() {
  case "$1" in
    /*) ;;
    *) fail "$2 must be an absolute path" ;;
  esac
  if printf '%s' "$1" | LC_ALL=C grep -q '[[:cntrl:]]'; then
    fail "$2 contains a control character"
  fi
  return 0
}

receipt_install_dir=
receipt_data_dir=
receipt_binary=
receipt_manuals=
receipt_version=
if [ -f "$receipt" ]; then
  [ "$(receipt_value schema)" = "$RECEIPT_SCHEMA" ] \
    || fail "installer receipt has an unsupported schema"
  receipt_install_dir=$(receipt_value install_dir)
  receipt_data_dir=$(receipt_value data_dir)
  receipt_binary=$(receipt_value binary)
  receipt_manuals=$(receipt_values manual)
  receipt_version=$(receipt_value version)
fi

install_dir=${install_dir_override:-${receipt_install_dir:-$default_install_dir}}
data_dir=${data_dir_override:-${receipt_data_dir:-$default_data_dir}}
validate_path "$install_dir" "install directory"
validate_path "$data_dir" "data directory"
binary_path="$install_dir/mant"

write_receipt() {
  installed_version=$1
  owned_manuals=$2
  require mkdir
  require mv
  mkdir -p "$state_dir"
  receipt_temporary="$state_dir/.install-receipt.$$"
  {
    printf 'schema\t%s\n' "$RECEIPT_SCHEMA"
    printf 'version\t%s\n' "$installed_version"
    printf 'install_dir\t%s\n' "$install_dir"
    printf 'data_dir\t%s\n' "$data_dir"
    printf 'binary\t%s\n' "$binary_path"
    saved_ifs=$IFS
    IFS='
'
    for owned_manual in $owned_manuals; do
      printf 'manual\t%s\n' "$owned_manual"
    done
    IFS=$saved_ifs
  } > "$receipt_temporary"
  chmod 0600 "$receipt_temporary"
  mv "$receipt_temporary" "$receipt"
}

uninstall_owned_files() {
  [ -f "$receipt" ] \
    || fail "no installer receipt was found; ManT was not installed by this script"
  validate_path "$receipt_binary" "receipt binary path"
  [ "$receipt_binary" = "$receipt_install_dir/mant" ] \
    || fail "installer receipt contains an invalid binary path"
  saved_ifs=$IFS
  IFS='
'
  for receipt_manual in $receipt_manuals; do
    validate_path "$receipt_manual" "receipt manual path"
    case "$receipt_manual" in
      "$receipt_data_dir"/mant.md|"$receipt_data_dir"/mant-ir.md|"$receipt_data_dir"/mant-markdown.md|"$receipt_data_dir"/mant-protocol.md|"$receipt_data_dir"/mant-roff.md) ;;
      *) fail "installer receipt contains an invalid manual path" ;;
    esac
  done
  IFS=$saved_ifs

  require rm
  removed=false
  if [ -e "$receipt_binary" ] || [ -L "$receipt_binary" ]; then
    rm -f "$receipt_binary"
    printf 'Removed %s\n' "$receipt_binary"
    removed=true
  fi
  saved_ifs=$IFS
  IFS='
'
  for receipt_manual in $receipt_manuals; do
    if [ -e "$receipt_manual" ] || [ -L "$receipt_manual" ]; then
      rm -f "$receipt_manual"
      printf 'Removed %s\n' "$receipt_manual"
      removed=true
    fi
  done
  IFS=$saved_ifs
  rm -f "$receipt"

  if [ "$removed" = true ]; then
    printf 'Uninstalled ManT %s\n' "${receipt_version:-}"
  else
    printf 'ManT files were already absent; removed the installer receipt.\n'
  fi
}

if [ "$uninstall" = true ]; then
  uninstall_owned_files
  exit 0
fi

[ "$host" != linux ] || require_glibc

require curl
require install
require mktemp

release_tag() {
  if [ -n "$requested_version" ]; then
    case "$requested_version" in
      v*) printf '%s\n' "$requested_version" ;;
      *) printf 'v%s\n' "$requested_version" ;;
    esac
    return
  fi

  latest_url=$(curl --proto '=https' --tlsv1.2 -fsSL \
    -o /dev/null -w '%{url_effective}' "$GITHUB_URL/releases/latest")
  printf '%s\n' "${latest_url##*/}"
}

download() {
  curl --proto '=https' --tlsv1.2 -fsSL "$1" -o "$2"
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    fail "sha256sum or shasum is required to verify the release"
  fi
}

verify_github_attestation() {
  artifact=$1
  if command -v gh >/dev/null 2>&1 \
    && gh auth status >/dev/null 2>&1; then
    gh attestation verify "$artifact" --repo "$REPOSITORY" >/dev/null \
      || fail "GitHub attestation verification failed for ${artifact##*/}"
    printf 'Verified GitHub provenance for %s\n' "${artifact##*/}"
  fi
}

verify_release_asset() {
  asset=$1
  checksums=$2
  asset_name=${asset##*/}
  expected=$(awk -v asset="$asset_name" \
    '($2 == asset || $2 == "*" asset) { print $1; exit }' "$checksums")
  [ -n "$expected" ] || fail "SHA256SUMS does not contain $asset_name"
  actual=$(sha256 "$asset")
  [ "$actual" = "$expected" ] \
    || fail "SHA-256 verification failed for $asset_name"
  verify_github_attestation "$asset"
}

installed_version() {
  [ -x "$binary_path" ] || return 0
  current_output=$("$binary_path" --version 2>/dev/null || :)
  printf '%s\n' "$current_output" \
    | awk '$1 == "mant" { print $2; exit }'
}

tag=$(release_tag)
printf '%s\n' "$tag" \
  | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$' \
  || fail "release tag '$tag' is not a supported version"
version=${tag#v}
current_version=$(installed_version)

case "$version" in
  0.[0-6].*)
    manual_names="mant.md"
    if [ "$host" = macos ] && [ "$install_manual" = true ]; then
      printf '%s\n' \
        "mant installer: release $tag predates verified manual bundles; installing the binary only" >&2
      install_manual=false
    fi
    ;;
  *) manual_names=$BUNDLED_MANUALS ;;
esac

owned_manuals=$receipt_manuals
if [ "$install_manual" = true ]; then
  owned_manuals=
  for manual_name in $manual_names; do
    owned_manuals="${owned_manuals}${owned_manuals:+
}$data_dir/$manual_name"
  done
elif [ -n "$receipt_data_dir" ] && [ "$receipt_data_dir" != "$data_dir" ]; then
  owned_manuals=
fi
manual_ready=true
if [ "$install_manual" = true ]; then
  for manual_name in $manual_names; do
    [ -f "$data_dir/$manual_name" ] || manual_ready=false
  done
fi

if [ "$force" = false ] \
  && [ "$current_version" = "$version" ] \
  && [ "$manual_ready" = true ]; then
  write_receipt "$version" "$owned_manuals"
  printf 'ManT %s is already up to date.\n' "$version"
  exit 0
fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/mant-install.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

install_files() {
  binary=$1
  manual_dir=$2
  mkdir -p "$install_dir"
  install -m 0755 "$binary" "$binary_path"
  if [ "$install_manual" = true ]; then
    mkdir -p "$data_dir"
    owned_manuals=
    for manual_name in $manual_names; do
      [ -f "$manual_dir/$manual_name" ] \
        || fail "manual bundle is missing $manual_name"
      install -m 0644 "$manual_dir/$manual_name" "$data_dir/$manual_name"
      owned_manuals="${owned_manuals}${owned_manuals:+
}$data_dir/$manual_name"
    done
  else
    retained_manuals=
    saved_ifs=$IFS
    IFS='
'
    for owned_manual in $owned_manuals; do
      if [ -f "$owned_manual" ]; then
        retained_manuals="${retained_manuals}${retained_manuals:+
}$owned_manual"
      fi
    done
    IFS=$saved_ifs
    owned_manuals=$retained_manuals
  fi
  write_receipt "$version" "$owned_manuals"

  if [ -z "$current_version" ]; then
    action=Installed
  elif [ "$current_version" = "$version" ]; then
    action=Reinstalled
  else
    action=Updated
  fi
  printf '%s ManT %s' "$action" "$version"
  if [ "$action" = Updated ]; then
    printf ' (from %s)' "$current_version"
  fi
  printf '\n  executable: %s\n' "$binary_path"
  if [ -n "$owned_manuals" ]; then
    printf '  manuals:    %s\n' "$data_dir"
  fi
  case ":${PATH:-}:" in
    *":$install_dir:"*) ;;
    *) printf '\nAdd %s to PATH, then run: mant mant\n' "$install_dir" ;;
  esac
}

install_linux() {
  case $(uname -m) in
    x86_64|amd64) target=linux-x64 ;;
    aarch64|arm64) target=linux-arm64 ;;
    *) fail "Linux architecture '$(uname -m)' has no prebuilt ManT release" ;;
  esac

  archive="mant-$version-$target.tar.gz"
  release_url="$GITHUB_URL/releases/download/$tag"
  download "$release_url/$archive" "$temporary/$archive"
  download "$release_url/SHA256SUMS" "$temporary/SHA256SUMS"
  verify_release_asset "$temporary/$archive" "$temporary/SHA256SUMS"

  require tar
  tar -xzf "$temporary/$archive" -C "$temporary"
  package="$temporary/mant-$version-$target"
  [ -x "$package/mant" ] || fail "$archive does not contain the mant executable"
  if [ -f "$package/manuals/manifest.txt" ]; then
    manual_dir="$package/manuals"
  else
    manual_names="mant.md"
    manual_dir=$package
  fi
  [ -f "$manual_dir/mant.md" ] || fail "$archive does not contain the ManT manuals"
  install_files "$package/mant" "$manual_dir"
}

install_macos() {
  require cargo
  cargo_root="$temporary/cargo-root"
  cargo install --locked --root "$cargo_root" --version "$version" mant
  manual_dir="$temporary/manuals"
  if [ "$install_manual" = true ]; then
    manual_archive="mant-$version-manuals.tar.gz"
    release_url="$GITHUB_URL/releases/download/$tag"
    download "$release_url/$manual_archive" "$temporary/$manual_archive"
    download "$release_url/SHA256SUMS" "$temporary/SHA256SUMS"
    verify_release_asset "$temporary/$manual_archive" "$temporary/SHA256SUMS"
    require tar
    tar -xzf "$temporary/$manual_archive" -C "$temporary"
    manual_dir="$temporary/mant-$version-manuals/manuals"
    [ -f "$manual_dir/manifest.txt" ] \
      || fail "$manual_archive does not contain a manual manifest"
  fi
  install_files "$cargo_root/bin/mant" "$manual_dir"
}

case "$host" in
  linux) install_linux ;;
  macos) install_macos ;;
esac
