#!/usr/bin/env bash
# Disassembly diff of mismatching functions in a kept win_parity DIFF object pair.
# usage: tools/win_fndiff.sh <diff-dir> [function-regex]
d="$1"; pat="${2:-.}"
OD="${REFCTX_OBJDUMP:-/c/Projects/FFCC-Decomp/build/binutils/powerpc-eabi-objdump.exe}"
dis() { "$OD" -dr --no-show-raw-insn "$1" | sed -E 's/^ +[0-9a-f]+:\t//; s/^\t+//'; }
split() { awk -v out="$2" '/^[0-9a-f]+ <.*>:$/ {name=$2; gsub(/[<>:]/,"",name); file=out"/"name; next} name {print > file}' <(dis "$1"); }
tmp=$(mktemp -d); mkdir -p "$tmp/r" "$tmp/o"
split "$d/ref.o" "$tmp/r"; split "$d/our.o" "$tmp/o"
for f in "$tmp"/r/*; do n=$(basename "$f"); [[ "$n" =~ $pat ]] || continue
  if ! cmp -s "$f" "$tmp/o/$n"; then echo "=== $n"; diff "$f" "$tmp/o/$n" | head -${LINES_MAX:-60}; fi; done
rm -rf "$tmp"
