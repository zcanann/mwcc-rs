#!/usr/bin/env bash
# Parity dashboard: run refctx over many reference TUs and tally the verdicts.
#
# For each .c under the given subtree, refctx classifies the whole-object result
# as BYTE (byte-identical ✅), DIFF (compiled, but bytes differ), or DEFER (our
# mwcc refused — an unimplemented construct). The summary line at the end is the
# parity score for that subtree.
#
# Usage:
#   tools/sweep.sh <project_dir> <subdir-relative-to-project> [version] [cflags…]
# Example:
#   tools/sweep.sh ../Metrowerks/reference_projects/marioparty4 src/MSL_C.PPCEABI.bare.H 2.6
#
# Runs wibo, so do NOT run it while a full oracle sweep is in progress.
set -euo pipefail

project="${1:?usage: sweep.sh <project_dir> <subdir> [version] [cflags…]}"
subdir="${2:?need a subdir relative to the project}"
version="${3:-2.6}"
shift $(( $# < 3 ? $# : 3 ))

here="$(cd "$(dirname "$0")" && pwd)"
byte=0; diff=0; defer=0; fail=0
diffs=(); defers=()

while IFS= read -r file; do
  rel="${file#"$project"/}"
  line="$("$here/refctx.sh" "$project" "$rel" "$version" "$@" 2>&1 | head -1 || true)"
  case "$line" in
    BYTE*)  byte=$((byte+1));;
    DIFF*)  diff=$((diff+1)); diffs+=("$rel");;
    DEFER*) defer=$((defer+1)); defers+=("$rel — ${line#DEFER*— }");;
    *)      fail=$((fail+1)); defers+=("$rel — HARNESS: $line");;
  esac
  printf '%s\n' "$line"
done < <(find "$project/$subdir" -name '*.c' | sort)

total=$((byte+diff+defer+fail))
echo "================================================================"
echo "== $subdir @ GC/$version: $byte/$total BYTE, $diff DIFF, $defer DEFER, $fail HARNESS =="
if ((${#defers[@]})); then printf '\n-- DEFER/HARNESS reasons --\n'; printf '  %s\n' "${defers[@]}"; fi
if ((${#diffs[@]})); then printf '\n-- DIFF files (compiled, bytes differ) --\n'; printf '  %s\n' "${diffs[@]}"; fi
