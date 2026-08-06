#!/bin/sh
# Install, update, or uninstall ManT for one Unix user.

set -eu

REPOSITORY=BryanHeBY/ManT
GITHUB_URL="https://github.com/$REPOSITORY"
RECEIPT_SCHEMA=mant.install/v1

fail() {
  printf 'mant installer: %s\n' "$1" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
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
  --no-manual              Do not install the bundled mant.md manual
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
receipt_manual=
receipt_version=
if [ -f "$receipt" ]; then
  [ "$(receipt_value schema)" = "$RECEIPT_SCHEMA" ] \
    || fail "installer receipt has an unsupported schema"
  receipt_install_dir=$(receipt_value install_dir)
  receipt_data_dir=$(receipt_value data_dir)
  receipt_binary=$(receipt_value binary)
  receipt_manual=$(receipt_value manual)
  receipt_version=$(receipt_value version)
fi

install_dir=${install_dir_override:-${receipt_install_dir:-$default_install_dir}}
data_dir=${data_dir_override:-${receipt_data_dir:-$default_data_dir}}
validate_path "$install_dir" "install directory"
validate_path "$data_dir" "data directory"
binary_path="$install_dir/mant"
manual_path="$data_dir/mant.md"

write_receipt() {
  installed_version=$1
  owned_manual=$2
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
    printf 'manual\t%s\n' "$owned_manual"
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
  if [ -n "$receipt_manual" ]; then
    validate_path "$receipt_manual" "receipt manual path"
    [ "$receipt_manual" = "$receipt_data_dir/mant.md" ] \
      || fail "installer receipt contains an invalid manual path"
  fi

  require rm
  removed=false
  if [ -e "$receipt_binary" ] || [ -L "$receipt_binary" ]; then
    rm -f "$receipt_binary"
    printf 'Removed %s\n' "$receipt_binary"
    removed=true
  fi
  if [ -n "$receipt_manual" ] \
    && { [ -e "$receipt_manual" ] || [ -L "$receipt_manual" ]; }; then
    rm -f "$receipt_manual"
    printf 'Removed %s\n' "$receipt_manual"
    removed=true
  fi
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

owned_manual=$receipt_manual
if [ "$install_manual" = true ]; then
  owned_manual=$manual_path
elif [ -n "$receipt_data_dir" ] && [ "$receipt_data_dir" != "$data_dir" ]; then
  owned_manual=
fi
manual_ready=true
if [ "$install_manual" = true ] && [ ! -f "$manual_path" ]; then
  manual_ready=false
fi

if [ "$force" = false ] \
  && [ "$current_version" = "$version" ] \
  && [ "$manual_ready" = true ]; then
  write_receipt "$version" "$owned_manual"
  printf 'ManT %s is already up to date.\n' "$version"
  exit 0
fi

temporary=$(mktemp -d "${TMPDIR:-/tmp}/mant-install.XXXXXX")
trap 'rm -rf "$temporary"' EXIT HUP INT TERM

install_files() {
  binary=$1
  manual=$2
  mkdir -p "$install_dir"
  install -m 0755 "$binary" "$binary_path"
  if [ "$install_manual" = true ]; then
    mkdir -p "$data_dir"
    install -m 0644 "$manual" "$manual_path"
  elif [ -n "$owned_manual" ] && [ ! -f "$owned_manual" ]; then
    owned_manual=
  fi
  write_receipt "$version" "$owned_manual"

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
  if [ -n "$owned_manual" ]; then
    printf '  manual:     %s\n' "$owned_manual"
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

  expected=$(awk -v archive="$archive" '$2 == archive { print $1 }' \
    "$temporary/SHA256SUMS")
  [ -n "$expected" ] || fail "SHA256SUMS does not contain $archive"
  actual=$(sha256 "$temporary/$archive")
  [ "$actual" = "$expected" ] || fail "SHA-256 verification failed for $archive"

  require tar
  tar -xzf "$temporary/$archive" -C "$temporary"
  package="$temporary/mant-$version-$target"
  [ -x "$package/mant" ] || fail "$archive does not contain the mant executable"
  [ -f "$package/mant.md" ] || fail "$archive does not contain the ManT manual"
  install_files "$package/mant" "$package/mant.md"
}

install_macos() {
  require cargo
  cargo_root="$temporary/cargo-root"
  cargo install --locked --root "$cargo_root" --version "$version" mant
  manual="$temporary/mant.md"
  download "https://raw.githubusercontent.com/$REPOSITORY/$tag/docs/manuals/mant.md" "$manual"
  install_files "$cargo_root/bin/mant" "$manual"
}

case "$host" in
  linux) install_linux ;;
  macos) install_macos ;;
esac
