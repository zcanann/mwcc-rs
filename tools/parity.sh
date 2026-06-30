#!/usr/bin/env bash
# Parity dashboard — mwcc-rs vs real mwcceppc across a reference project's source TUs.
#
# mwcc-rs is BYTE-EXACT-OR-DEFER: it either emits an object byte-identical to mwcc (PERFECT) or
# declines (DEFER). There is no "fuzzy" middle by design, so DIFF (compiled but differs) must stay
# ~0 — a non-zero DIFF is a correctness bug, not a near-miss. The "how close" signal is therefore the
# DEFER rate and WHY: a PARSER defer is far from matching (mwcc-rs can't even read the TU); a CODEGEN
# defer is one modelled feature away (it parsed, the lowering isn't done); both are tracked below.
#
# Usage: tools/parity.sh <project_dir> [version] [extra cflags…]
#   tools/parity.sh ../Metrowerks/reference_projects/marioparty4 2.6
# Slow (decompctx + mwcc -E + two compiles per TU); run in the background for a whole project.
set -uo pipefail
project="${1:?usage: parity.sh <project_dir> [version] [cflags…]}"
version="${2:-2.6}"; shift $(( $# < 2 ? $# : 2 ))
here="$(cd "$(dirname "$0")" && pwd)"
perfect=0; diff=0; defer=0; harness=0; total=0
parser_defer=0; codegen_defer=0
while IFS= read -r file; do
  total=$((total+1))
  rel="${file#"$project"/}"
  line="$("$here/refctx.sh" "$project" "$rel" "$version" "$@" 2>&1 | head -1 || true)"
  case "$line" in
    BYTE*)  perfect=$((perfect+1));;
    DIFF*)  diff=$((diff+1)); echo "  !! DIFF (must be 0): $rel" >&2;;
    DEFER*) defer=$((defer+1))
      r="${line#*— }"
      case "$r" in
        *"expected "*|*"unexpected character"*|*"found "*|*"a type"*|*"is not a "*|*declspec*)
          parser_defer=$((parser_defer+1));;
        *) codegen_defer=$((codegen_defer+1));;
      esac;;
    *) harness=$((harness+1));;
  esac
  printf '\r  %d TUs | %d perfect  %d defer  %d diff  %d harness' "$total" "$perfect" "$defer" "$diff" "$harness" >&2
done < <(find "$project/src" -name '*.c' | sort)
echo >&2; echo
pct(){ awk "BEGIN{printf \"%.1f\", ($1)*100/($2==0?1:$2)}"; }
echo "════ PARITY: $(basename "$project") @ GC/$version  ($total TUs) ════"
echo "  PERFECT (byte-identical to mwcc) : $perfect  ($(pct $perfect $total)%)"
echo "  DEFER   (mwcc-rs declines)       : $defer  ($(pct $defer $total)%)"
echo "  DIFF    (compiled but differs)   : $diff  <- byte-exact-or-defer => must be 0"
echo "  HARNESS (toolchain/decompctx err): $harness"
echo "  ── of the DEFERs ──"
echo "    parser  (can't read the TU)    : $parser_defer  ($(pct $parser_defer $total)% of TUs)"
echo "    codegen (parsed, lowering TODO): $codegen_defer  ($(pct $codegen_defer $total)% of TUs)"
