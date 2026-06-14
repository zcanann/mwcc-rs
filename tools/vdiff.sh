#!/usr/bin/env bash
# vdiff.sh — compare the .text mwcceppc emits for one source across two builds.
#
# Usage:  tools/vdiff.sh <file.c> <verA> <verB>
#         echo 'int f(int a){return a;}' | tools/vdiff.sh - 1.3 1.3.2
#
# Prints SAME or DIFF; on DIFF, prints the per-instruction disassembly of both
# (verA on the left, verB on the right) so a divergence is obvious. This is the
# instrument for mapping where two real compiler builds disagree byte-for-byte.
set -euo pipefail

DECOMP="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
WIBO="$DECOMP/build/tools/wibo"
SJIS="$DECOMP/build/tools/sjiswrap.exe"
OBJDUMP="$DECOMP/build/binutils/powerpc-eabi-objdump"
FLAGS=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware -O4,p
       -inline auto -maxerrors 1 -nosyspath -RTTI off -fp_contract on -str reuse -lang=c)

src="$1"; verA="$2"; verB="$3"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
if [ "$src" = "-" ]; then cat > "$tmp/in.c"; src="$tmp/in.c"; fi

disasm() { # <version> -> per-instruction "bytes  mnemonic" lines
  local ver="$1" obj="$tmp/$1.o"
  "$WIBO" "$SJIS" "$DECOMP/build/compilers/GC/$ver/mwcceppc.exe" "${FLAGS[@]}" \
      -c "$src" -o "$obj" >/dev/null 2>&1 || true
  [ -f "$obj" ] || { echo "(REJECTED by $ver)"; return; }
  "$OBJDUMP" -d -j .text "$obj" 2>/dev/null \
    | awk -F'\t' 'NF==3{gsub(/ /,"",$2); printf "%-12s %s\n",$2,$3}'
}

a="$(disasm "$verA")"; b="$(disasm "$verB")"
if [ "$a" = "$b" ]; then
  echo "SAME ($verA == $verB)"
else
  echo "DIFF ($verA | $verB)"
  paste -d'|' <(printf '%s\n' "$a") <(printf '%s\n' "$b") \
    | awk -F'|' '{ if ($1!=$2) printf "  %-34s | %s\n",$1,$2 }'
fi
