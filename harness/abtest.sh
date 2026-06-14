#!/bin/bash
# A/B test harness: compile each corpus .c with BOTH the real mwcceppc (oracle)
# and our mwcc-rs, then diff the .text disassembly. The real compiler is the
# source of truth; we are correct iff our .text matches byte-for-byte.
#
# Usage: harness/abtest.sh [GC_VERSION]   (default 1.3.2)
set -u
HERE="$(cd "$(dirname "$0")/.." && pwd)"
FF="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
VER="${1:-1.3.2}"

WIBO="$FF/build/tools/wibo"
SJIS="$FF/build/tools/sjiswrap.exe"
MWCC="$FF/build/compilers/GC/$VER/mwcceppc.exe"
OBJD="$FF/build/binutils/powerpc-eabi-objdump"
OURS="$HERE/target/release/mwcc"

# Flags mirror FFCC's game TUs but in plain C (-lang=c).
CF='-nodefaults -proc gekko -align powerpc -enum int -fp hardware -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off -fp_contract on -str reuse -lang=c'

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
disasm(){ "$OBJD" -d -j .text "$1" 2>/dev/null | sed -E 's/^[ ]*[0-9a-f]+:[[:space:]]+[0-9a-f ]+[[:space:]]+//' | grep -vE '^$|file format|Disassembly|^[0-9a-f]+ <'; }

pass=0; fail=0
echo "== A/B vs mwcceppc GC/$VER =="
for c in "$HERE"/corpus/*.c; do
  [ -f "$c" ] || continue
  base="$(basename "$c" .c)"
  # oracle
  eval "$WIBO" "$SJIS" "$MWCC" $CF -c "$c" -o "$tmp/ref.o" 2>/dev/null
  # ours
  "$OURS" -c "$c" -o "$tmp/got.o" 2>"$tmp/err"
  if [ ! -f "$tmp/ref.o" ]; then echo "  SKIP $base (oracle rejected)"; continue; fi
  if [ ! -f "$tmp/got.o" ]; then echo "  FAIL $base (ours: $(tr '\n' ' ' <"$tmp/err"))"; fail=$((fail+1)); rm -f "$tmp/got.o"; continue; fi
  if diff -q <(disasm "$tmp/ref.o") <(disasm "$tmp/got.o") >/dev/null; then
    echo "  PASS $base"; pass=$((pass+1))
  else
    echo "  FAIL $base — diff (< ours | > oracle):"
    diff <(disasm "$tmp/got.o") <(disasm "$tmp/ref.o") | sed 's/^/      /'
    fail=$((fail+1))
  fi
  rm -f "$tmp/ref.o" "$tmp/got.o"
done
echo "== $pass passed, $fail failed =="
[ "$fail" -eq 0 ]
