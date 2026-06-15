#!/usr/bin/env bash
# Non-contending single-snippet probe: compile one C snippet with the real
# mwcceppc and (optionally) our mwcc, print the .text disassembly of each.
#
# Unlike the oracle it writes to a PRIVATE mktemp dir, so it is safe to run
# while a full oracle sweep is in progress (no shared /tmp/mwcc-oracle files).
#
# Usage:
#   tools/probe.sh '<C source>'            # default build 1.3.2, real only
#   tools/probe.sh '<C source>' 2.7        # pick a build
#   tools/probe.sh '<C source>' 1.3.2 ours # also run our mwcc and diff
#
# The same -O4 flag set the oracle uses is applied.
set -euo pipefail

src="${1:?usage: probe.sh '<C source>' [version] [ours]}"
version="${2:-1.3.2}"
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

dir="$(mktemp -d "${TMPDIR:-/tmp}/mwprobe.XXXXXX")"
trap 'rm -rf "$dir"' EXIT
printf '%s\n' "$src" > "$dir/s.c"

"$wibo" "$sjis" "$compiler" "${flags[@]}" -c "$dir/s.c" -o "$dir/ref.o" >/dev/null 2>&1 || true
if [[ -f "$dir/ref.o" ]]; then
  echo "=== real mwcceppc GC/$version ==="
  "$objdump" -d "$dir/ref.o" | sed -n '/<.*>:/,/^$/p'
else
  echo "(real compiler rejected the source)"
fi

if [[ "$mode" == "ours" ]]; then
  "$ours_bin" --build "GC/$version" -c "$dir/s.c" -o "$dir/ours.o" 2>"$dir/err" || true
  echo "=== our mwcc GC/$version ==="
  if [[ -f "$dir/ours.o" ]]; then
    "$objdump" -d "$dir/ours.o" | sed -n '/<.*>:/,/^$/p'
  else
    echo "(ours deferred: $(cat "$dir/err"))"
  fi
fi
