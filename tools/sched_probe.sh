#!/usr/bin/env bash
# Capture the real mwcc -O4,p instruction ORDER for one C snippet — the
# scheduler-model dataset instrument (fire 275: the interleave is core -O4
# selection; this records the linearization ground truth per shape).
#
# Usage: tools/sched_probe.sh '<C source>' [version]   (function must be `f`)
set -euo pipefail
src="${1:?usage: sched_probe.sh '<C source>' [version]}"
version="${2:-2.6}"
FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
dir="$(mktemp -d "${TMPDIR:-/tmp}/schedprobe.XXXXXX")"
trap 'rm -rf "$dir"' EXIT
printf '%s\n' "$src" > "$dir/s.c"
( cd "$dir" && "$FFCC/build/tools/wibo" "$FFCC/build/tools/sjiswrap.exe" \
  "$FFCC/build/compilers/GC/$version/mwcceppc.exe" \
  -nodefaults -proc gekko -align powerpc -enum int -fp hardware -O4,p \
  -inline auto -maxerrors 1 -nosyspath -RTTI off -fp_contract on -str reuse \
  -lang=c -c s.c -o s.o ) 2>/dev/null
"$FFCC/build/binutils/powerpc-eabi-objdump" -d "$dir/s.o" \
  | sed -n '/<f>:/,/^$/p' | grep -E "^\s+[0-9a-f]+:" \
  | awk '{for(i=6;i<=NF;i++)printf "%s ",$i; print ""}' | sed 's/ *$//'
