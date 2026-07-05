#!/usr/bin/env bash
# Combined compiler-support parity across all reference projects.
#
# The historical gate swept only each project's MSL C subtree. But a "fully
# functioning mwcc" must also compile the RUNTIME support (global_destructor_chain,
# __mem, __va_arg, …) — which lives in a separate `Runtime/` tree the MSL sweep
# never touched. This walks BOTH the MSL and Runtime subtrees per project and
# reports a combined BYTE / DIFF / DEFER tally, so the ~16 Runtime wins (gdc,
# __mem.c) are credited and future runtime work is measured.
#
# DIFF MUST STAY 0 (byte-exact-or-defer). Usage: tools/support_parity.sh [2.6]
set -uo pipefail
version="${1:-2.6}"
here="$(cd "$(dirname "$0")" && pwd)"
R="$here/../../Metrowerks/reference_projects"

# project | MSL subtree | Runtime subtree (dir holding global_destructor_chain.c)
ROWS=(
  "marioparty4|src/MSL_C.PPCEABI.bare.H|src/Runtime.PPCEABI.H"
  "pikmin|src/MSL_C|src/Runtime/PPCEABI/H"
  "super_smash_brothers_melee|src/MSL|src/Runtime"
  "animal_crossing|src/static/MSL_C.PPCEABI.bare.H|src/static/Runtime.PPCEABI.H"
  "battle_for_bikini_bottom|src/PowerPC_EABI_Support/src/MSL_C|src/PowerPC_EABI_Support/src/Runtime"
  "super_mario_strikers|src/PowerPC_EABI_Support/MSL|src/PowerPC_EABI_Support/Runtime"
  "pikmin2|src/Dolphin/MSL_C|src/Dolphin/Runtime"
  "super_mario_sunshine|src/PowerPC_EABI_Support/Msl/MSL_C|src/PowerPC_EABI_Support/Runtime"
  "wind_waker|src/PowerPC_EABI_Support/MSL/MSL_C|src/PowerPC_EABI_Support/Runtime/Src"
)

tot_byte=0; tot_diff=0; tot_defer=0
for row in "${ROWS[@]}"; do
  IFS='|' read -r project msl runtime <<< "$row"
  b=0; d=0; f=0
  for sub in "$msl" "$runtime"; do
    [[ -d "$R/$project/$sub" ]] || continue
    out="$("$here/sweep.sh" "$R/$project" "$sub" "$version" 2>&1 || true)"
    b=$((b + $(grep -c '^BYTE' <<< "$out")))
    d=$((d + $(grep -c '^DIFF' <<< "$out")))
    f=$((f + $(grep -c '^DEFER' <<< "$out")))
    grep '^DIFF' <<< "$out" | sed 's/^/  !! /' >&2
  done
  printf '%-28s %3dB / %dD / %3dDEFER\n' "$project" "$b" "$d" "$f"
  tot_byte=$((tot_byte+b)); tot_diff=$((tot_diff+d)); tot_defer=$((tot_defer+f))
done
echo "-----------------------------------------------"
printf 'SUPPORT TOTAL                %3dB / %dD / %3dDEFER\n' "$tot_byte" "$tot_diff" "$tot_defer"
[[ "$tot_diff" -eq 0 ]] || { echo "FAIL: DIFF must be 0"; exit 1; }
