#!/usr/bin/env bash
# racase — register-allocator (and any-codegen) COVERAGE PROBE via synthetic C.
#
# Compiles a one-line C snippet with BOTH the real mwcceppc and our mwcc at the
# marioparty4 2.6 flag set, and reports BYTE / DIFF / DEFER. This is how the
# register-allocator boundary was mapped (single value live across a call = BYTE;
# 3+ values, multiply-combine, long long = DEFER) without needing a matching
# reference file in a project. Use it to (a) find the simplest shape past current
# coverage, (b) grab the exact target bytes to build a handler against.
#
# Usage:
#   tools/racase.sh '<name>' '<one-line C source>'
#   tools/racase.sh --dis '<name>' '<C>'   # also print the real disassembly of `f`
# Example:
#   tools/racase.sh mul 'int g(void); int f(int a,int b){ g(); return a*b; }'
set -euo pipefail
FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
wibo="$FFCC/build/tools/wibo"; sjis="$FFCC/build/tools/sjiswrap.exe"
compiler="$FFCC/build/compilers/GC/2.6/mwcceppc.exe"
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
here="$(cd "$(dirname "$0")/.." && pwd)"; ours="$here/target/release/mwcc"
base=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware \
  -Cpp_exceptions off -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off \
  -fp_contract on -str reuse)
show_dis=0; [[ "${1:-}" == "--dis" ]] && { show_dis=1; shift; }
name="${1:?need a case name}"; body="${2:?need a C body}"
dir="$(mktemp -d "${TMPDIR:-/tmp}/racase.XXXXXX")"
trap 'case "$dir" in */racase.??????) rm -rf "$dir";; esac' EXIT
printf '%s\n' "$body" > "$dir/c.c"
"$wibo" "$sjis" "$compiler" "${base[@]}" -c "$dir/c.c" -o "$dir/real.o" 2>/dev/null
"$wibo" "$sjis" "$compiler" "${base[@]}" -E "$dir/c.c" -o "$dir/c.i" 2>/dev/null
mkdir -p "$dir/ours" && cp "$dir/c.i" "$dir/ours/c.c"
if "$ours" --build GC/2.6 "${base[@]}" -c "$dir/ours/c.c" -o "$dir/our.o" 2>"$dir/e"; then
  if cmp -s "$dir/our.o" "$dir/real.o"; then verdict="BYTE ✅"
  else verdict="DIFF ($(cmp -l "$dir/our.o" "$dir/real.o" | wc -l | tr -d ' ') bytes)"; fi
else verdict="DEFER — $(sed 's/^mwcc: //' "$dir/e" | head -1)"; fi
printf '%-24s %s\n' "$name:" "$verdict"
[[ "$show_dis" == 1 ]] && "$objdump" -d "$dir/real.o" 2>/dev/null | awk '/<f>:/,/blr/'
exit 0
