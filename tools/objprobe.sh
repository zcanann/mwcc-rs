#!/usr/bin/env bash
# Whole-object single-snippet probe: compile one C snippet with the real
# mwcceppc and our mwcc, then compare the ENTIRE object (section headers,
# symbols, and a full hex dump of every section) — not just `.text`.
#
# This is the data-section counterpart to probe.sh: use it when the difference
# of interest lives in `.sdata2`/`.rodata`/`.sdata`/`.sbss`/symbols, not code.
#
# Writes to a PRIVATE mktemp dir, so it is safe alongside an oracle sweep.
#
# Usage:
#   tools/objprobe.sh '<C source>'            # default build 2.6, real only
#   tools/objprobe.sh '<C source>' 2.6 ours   # also run ours and diff
set -euo pipefail

src="${1:?usage: objprobe.sh '<C source>' [version] [ours]}"
version="${2:-2.6}"
mode="${3:-real}"

FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
wibo="$FFCC/build/tools/wibo"
sjis="$FFCC/build/tools/sjiswrap.exe"
compiler="$FFCC/build/compilers/GC/$version/mwcceppc.exe"
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
here="$(cd "$(dirname "$0")/.." && pwd)"
ours_bin="$here/target/release/mwcc"

flags=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware \
  -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off \
  -fp_contract on -str reuse -lang=c)

dir="$(mktemp -d "${TMPDIR:-/tmp}/mwobjprobe.XXXXXX")"
trap 'rm -rf "$dir"' EXIT
printf '%s\n' "$src" > "$dir/s.c"

# A compact, diff-friendly rendering of an object: section sizes, the symbol
# table (minus volatile address columns), and a hex dump of all sections.
render() {
  local obj="$1"
  "$objdump" -h "$obj" | awk '/^ *[0-9]+ /{printf "SEC %-12s %s\n",$2,$3}'
  echo "-- symbols --"
  "$objdump" -t "$obj" | grep -E ' (g|l|w) ' | sed -E 's/^[0-9a-f]+ //'
  echo "-- contents --"
  "$objdump" -s "$obj" 2>/dev/null | sed -n '/Contents of section/,$p'
}

"$wibo" "$sjis" "$compiler" "${flags[@]}" -c "$dir/s.c" -o "$dir/ref.o" >/dev/null 2>&1 || true
if [[ -f "$dir/ref.o" ]]; then
  render "$dir/ref.o" > "$dir/ref.txt"
else
  echo "(real compiler rejected the source)"; exit 0
fi

if [[ "$mode" != "ours" ]]; then
  cat "$dir/ref.txt"; exit 0
fi

"$ours_bin" --build "GC/$version" -c "$dir/s.c" -o "$dir/ours.o" 2>"$dir/err" || true
if [[ ! -f "$dir/ours.o" ]]; then
  echo "(ours deferred: $(cat "$dir/err"))"; exit 0
fi
render "$dir/ours.o" > "$dir/ours.txt"

if cmp -s "$dir/ref.o" "$dir/ours.o"; then
  echo "BYTE-EXACT ✅  ($src)"
else
  echo "DIFFERS ❌  ($src)"
  echo "--- real (<) vs ours (>) ---"
  diff "$dir/ref.txt" "$dir/ours.txt" || true
fi
