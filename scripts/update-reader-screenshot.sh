#!/usr/bin/env bash
# Regenerate the README reader screenshot in an isolated graphical terminal.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

usage() {
  cat <<'EOF'
Regenerate the ManT reader screenshot.

Usage:
  scripts/update-reader-screenshot.sh [OUTPUT]

OUTPUT defaults to docs/assets/screenshots/mant-reader.png.

Linux dependencies: Xvfb, xterm, xdotool, Fontconfig, and ImageMagick.
EOF
}

fail() {
  printf 'screenshot: %s\n' "$1" >&2
  exit 1
}

require() {
  command -v "$1" >/dev/null 2>&1 || fail "required command '$1' was not found"
}

case ${1:-} in
  -h|--help)
    usage
    exit 0
    ;;
esac
[[ $# -le 1 ]] || fail "expected at most one output path"

output=${1:-docs/assets/screenshots/mant-reader.png}
if [[ $output != /* ]]; then
  output="$ROOT/$output"
fi

if [[ ${MANT_SCREENSHOT_XVFB:-} != 1 ]]; then
  require xvfb-run
  exec env MANT_SCREENSHOT_XVFB=1 \
    xvfb-run -a -s '-screen 0 1800x1200x24 -dpi 96' \
    "$ROOT/scripts/update-reader-screenshot.sh" "$output"
fi

for tool in cargo fc-cache fc-match import magick xdotool xterm; do
  require "$tool"
done

font_dir="$ROOT/docs/assets/fonts"
font_pattern='JetBrains Mono'
for file in \
  JetBrainsMono-Regular.ttf \
  JetBrainsMono-Medium.ttf \
  JetBrainsMono-Bold.ttf \
  JetBrainsMono-Italic.ttf \
  JetBrainsMono-BoldItalic.ttf \
  fonts.conf \
  OFL.txt; do
  [[ -f "$font_dir/$file" ]] || fail "missing pinned font asset docs/assets/fonts/$file"
done

printf '==> build release executable\n'
CCACHE_DISABLE=${CCACHE_DISABLE:-1} cargo build --locked --release --package mant
mant="$ROOT/target/release/mant"
[[ -x $mant ]] || fail "Cargo did not produce $mant"

temporary=$(mktemp -d "${TMPDIR:-/tmp}/mant-screenshot.XXXXXX")
terminal_pid=
output_temporary="$output.tmp.$$"
cleanup() {
  if [[ -n $terminal_pid ]] && kill -0 "$terminal_pid" 2>/dev/null; then
    kill "$terminal_pid" 2>/dev/null || true
    wait "$terminal_pid" 2>/dev/null || true
  fi
  rm -f "$output_temporary"
  rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

home="$temporary/home"
data_home="$temporary/data"
cache_home="$temporary/cache"
config_home="$temporary/config"
mkdir -p \
  "$home" \
  "$data_home/mant/documents" \
  "$cache_home" \
  "$config_home" \
  "$(dirname "$output")"

install -m 0644 docs/manuals/mant.md "$data_home/mant/documents/mant.md"

runtime_environment=(
  "HOME=$home"
  "XDG_DATA_HOME=$data_home"
  "XDG_CACHE_HOME=$cache_home"
  "XDG_CONFIG_HOME=$config_home"
  "FONTCONFIG_FILE=$font_dir/fonts.conf"
  "LANG=C.UTF-8"
  "LC_ALL=C.UTF-8"
  "TERM=xterm-256color"
  "COLORTERM=truecolor"
)

# Screenshot output must keep the application's theme even when the caller
# follows NO_COLOR for its own terminal session.
unset NO_COLOR

printf '==> prepare pinned JetBrains Mono Medium font\n'
env "${runtime_environment[@]}" fc-cache -f "$font_dir" >/dev/null
matched_normal_font=$(env "${runtime_environment[@]}" fc-match -f '%{file}\n' "$font_pattern")
[[ $matched_normal_font == "$font_dir/JetBrainsMono-Medium.ttf" ]] \
  || fail "Fontconfig did not select the pinned JetBrains Mono Medium font"
matched_bold_font=$(env "${runtime_environment[@]}" fc-match -f '%{file}\n' \
  'JetBrains Mono:style=Bold')
[[ $matched_bold_font == "$font_dir/JetBrainsMono-Bold.ttf" ]] \
  || fail "Fontconfig did not select the pinned JetBrains Mono Bold font"

printf '==> start isolated ManT reader\n'
env "${runtime_environment[@]}" xterm \
  -name mant-screenshot \
  -title mant-screenshot \
  -geometry 135x47 \
  -fa "$font_pattern" \
  -fs 14 \
  -bg '#11111b' \
  -fg '#cdd6f4' \
  -b 0 \
  -xrm 'XTerm*renderFont: true' \
  -xrm 'XTerm*directColor: true' \
  -xrm 'XTerm*allowBoldFonts: true' \
  -xrm 'XTerm*scrollBar: false' \
  -xrm 'XTerm*toolBar: false' \
  -xrm 'XTerm*internalBorder: 0' \
  -xrm 'XTerm*borderWidth: 0' \
  -xrm 'XTerm*cursorBlink: false' \
  -e "$mant" mant &
terminal_pid=$!

window=
for _ in {1..200}; do
  if candidates=$(xdotool search --onlyvisible --name '^mant-screenshot$' 2>/dev/null); then
    window=${candidates%%$'\n'*}
    [[ -n $window ]] && break
  fi
  if ! kill -0 "$terminal_pid" 2>/dev/null; then
    wait "$terminal_pid" || true
    fail "ManT reader exited before its xterm window appeared"
  fi
  sleep 0.05
done
[[ -n $window ]] || fail "timed out waiting for the xterm window"

# F10 opens File; Right selects View; Down twice selects Expand All.
sleep 0.5
xdotool windowfocus --sync "$window"
xdotool key --clearmodifiers F10 Right Down Down Return
sleep 0.5

printf '==> capture expanded reader\n'
captured="$temporary/mant-reader.png"
import -silent -window "$window" "$captured"
magick "$captured" -strip "PNG:$output_temporary"
chmod 0644 "$output_temporary"
mv "$output_temporary" "$output"

dimensions=$(magick identify -format '%wx%h' "$output")
printf 'updated %s (%s)\n' "$output" "$dimensions"

xdotool windowfocus --sync "$window" 2>/dev/null || true
xdotool key --clearmodifiers q 2>/dev/null || true
for _ in {1..20}; do
  kill -0 "$terminal_pid" 2>/dev/null || break
  sleep 0.05
done
if kill -0 "$terminal_pid" 2>/dev/null; then
  kill "$terminal_pid" 2>/dev/null || true
fi
wait "$terminal_pid" 2>/dev/null || true
terminal_pid=
