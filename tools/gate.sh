#!/usr/bin/env bash
# The canonical pre-commit gate — one command for the full byte-exact-or-defer check.
#
# Runs the differential oracle for every version whose codegen we model to green,
# the vreg unit tests, and the reference-project support parity. The HARD INVARIANT
# is byte-exact-or-defer: every oracle version must be N/0 and support DIFF must be 0.
#
# 2.0p1 is INCLUDED deliberately: it is the ONLY build exercising the Gc20Patch1
# float-scheduling knobs (lr_save_precedes_float_const, float_compare_value_before_const,
# frexp_scale_before_eptr_store), so it is their sole regression protection — the
# mainline oracle runs leave those knobs false. (2.5/2.7 are mainline-identical to 2.6;
# GC/1.3 is intentionally omitted — its char-unsigned residuals are keystone-gated.)
#
# Usage: tools/gate.sh          # full gate
#        tools/gate.sh --quick  # skip the oracle (vreg + support only, ~1 min)
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
quick=0
[[ "${1:-}" == "--quick" ]] && quick=1
fail=0

run() { # $1=label  $2...=command
  local label="$1"; shift
  local out; out="$("$@" 2>&1)"
  if [[ "$label" == oracle* ]]; then
    local line; line="$(grep -E '^== [0-9]' <<<"$out" | tail -1)"
    if grep -qE '== [0-9]+ passed, 0 failed' <<<"$line"; then
      printf '  PASS  %-16s %s\n' "$label" "$line"
    else
      printf '  FAIL  %-16s %s\n' "$label" "${line:-<no summary — build/harness error>}"
      grep -E '^  FAIL ' <<<"$out" | head -5 | sed 's/^/          /'
      fail=1
    fi
  fi
}

echo "== gate: cargo build --release =="
if ! cargo build --release -q 2>/tmp/gate_build.err; then
  grep -E '^error' /tmp/gate_build.err | head; echo "  FAIL  build"; exit 1
fi

echo "== gate: vreg unit tests =="
vreg_out="$(cargo test --release -q -p mwcc-vreg 2>&1)"
if grep -qE 'test result: ok' <<<"$vreg_out" && ! grep -qE 'test result: FAILED|^error' <<<"$vreg_out"; then
  echo "  PASS  vreg      $(grep -E 'test result: ok\. [1-9]' <<<"$vreg_out" | head -1)"
else
  echo "  FAIL  vreg"; grep -E 'FAILED|^error' <<<"$vreg_out" | head -5 | sed 's/^/          /'; fail=1
fi

if [[ $quick -eq 0 ]]; then
  echo "== gate: differential oracle =="
  for v in 1.3.2 2.0 2.0p1 2.6 2.7; do
    run "oracle $v" cargo run --release -q -p mwcc-oracle -- "$v"
  done
fi

echo "== gate: reference support parity (2.6) =="
sp="$(tools/support_parity.sh 2.6 2>&1)"
sptot="$(grep 'SUPPORT TOTAL' <<<"$sp")"
# Byte-exact-or-defer requires DIFF == 0. But a change can also silently REGRESS
# coverage (flip BYTE -> DEFER) while keeping DIFF 0 — that passed as GREEN once
# (a volatile-local attempt dropped 454B -> 201B/253DEFER, gate none the wiser).
# So also assert a BYTE-count FLOOR: coverage must not drop below the known-good
# 454. Raise this floor when new support coverage lands; never lower it silently.
SUPPORT_BYTE_FLOOR=454
spbyte="$(grep -oE '[0-9]+B' <<<"$sptot" | tr -d 'B')"
if ! grep -qE '/ 0D /' <<<"$sptot"; then
  echo "  FAIL  support   $sptot  (DIFF != 0 — byte-exact-or-defer violation)"; fail=1
elif [[ -z "$spbyte" || "$spbyte" -lt "$SUPPORT_BYTE_FLOOR" ]]; then
  echo "  FAIL  support   $sptot  (BYTE coverage ${spbyte:-?} < floor $SUPPORT_BYTE_FLOOR — a BYTE->DEFER regression)"; fail=1
else
  echo "  PASS  support   $sptot"
fi

echo "-----------------------------------------------"
if [[ $fail -eq 0 ]]; then echo "GATE: GREEN"; else echo "GATE: FAILED"; fi
exit $fail
