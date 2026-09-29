#!/usr/bin/env bash
# Side-by-side reference vs PCode-pipeline disassembly of one canary function.
# usage: tools/win_pcode_show.sh <canary file> <function> [build]
here="$(cd "$(dirname "$0")/.." && pwd)"
src="$1"; fn="$2"; build="${3:-GC/2.6}"
flags=$(python3 -c "import sys;sys.path.insert(0,r'$(cd "$here" && pwd -W)/tools');import win_pcode_eval as e;from pathlib import Path;t=Path(r'$src').read_text(errors='replace');f=e.flags_for(t);print(' '.join(f+(['-lang=c'] if '$src'.endswith('.c') else [])))")
comp=$(python3 -c "import sys;sys.path.insert(0,r'$(cd "$here" && pwd -W)/tools');import win_parity as w;print(w.reference_compiler('$build'))")
OD=/c/Projects/FFCC-Decomp/build/binutils/powerpc-eabi-objdump.exe
tmp=$(mktemp -d)
"$comp" $flags -c "$src" -o $tmp/r.o >/dev/null 2>&1
MWCC_EXPERIMENTAL_BUILDS=1 MWCC_PCODE=only "$here/target/release/mwcc.exe" --build $build --parity-keep-going $flags -c "$src" -o $tmp/o.o 2>&1 | grep "'$fn'"
dis() { "$OD" -dr --no-show-raw-insn "$1" | awk -v f="<$fn>:" '$0 ~ f {p=1; next} /^[0-9a-f]+ </ {p=0} p' | sed -E 's/^ +[0-9a-f]+:\t//; s/^\t+//; s/\t/ /g' | grep -v "^$"; }
paste <(dis $tmp/r.o) <(dis $tmp/o.o) | awk -F'\t' '{printf "%-34s | %s\n", $1, $2}'
grep -m1 -B2 -A12 "$fn *(" "$src" | head -16
rm -rf $tmp
