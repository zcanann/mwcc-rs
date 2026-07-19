#!/usr/bin/env bash
# Optional exhaustive regression gate. This is not the parity metric and the
# frontier development loop does not run it on every change; use parity_loop.py
# to spend iteration time on known failures, untested configurations, and a
# small rotating audit of previous byte matches.
#
# Runs the differential oracle for every version whose codegen we model to green,
# the vreg unit tests, and the reference-project support parity. The HARD INVARIANT
# is byte-exact-or-defer: every oracle version must be N/0 and support DIFF must be 0.
#
# 2.0p1 is INCLUDED deliberately: it is the ONLY build exercising the Gc20Patch1
# float-scheduling knobs (lr_save_precedes_float_const, float_compare_value_before_const,
# frexp_scale_before_eptr_store), so it is their sole regression protection — the
# mainline oracle runs leave those knobs false. (2.5/2.7 are mainline-identical to 2.6.)
# GC/1.3.2r is deliberately excluded: it was a hacked Animal Crossing compiler
# build used to disable rodata pooling, and is not a required parity target.
#
# Usage: tools/gate.sh          # full gate
#        tools/gate.sh --quick  # skip the oracle (vreg + reference gates, ~1 min)
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
  run "oracle 1.2.5n" env MWCC_EXPERIMENTAL_BUILDS=1 cargo run --release -q -p mwcc-oracle -- 1.2.5n
  for v in 1.3 1.3.2 2.0 2.0p1 2.6 2.7; do
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

echo "== gate: exact configured Runtime parity (GC/2.6) =="
exact_out="$(python3 tools/reference_parity.py \
  --write-inventory target/reference-parity/gate-inventory.json \
  --compiler target/release/mwcc \
  --project marioparty4 --version GC/2.6 --language c \
  --source 'Runtime\.PPCEABI\.H/(GCN_Mem_Alloc|__mem|runtime|__va_arg|global_destructor_chain)\.c$' \
  --rerun --cache target/reference-parity/gate-runtime.jsonl 2>&1)"
exact_summary="$(grep -E '^== [0-9]+ configurations:' <<<"$exact_out" | tail -1)"
if grep -qF '== 30 configurations: BYTE 30 / DIFF 0 / DEFER 0 / HARNESS 0 / MISSING_DEPENDENCY 0 / UNSUPPORTED_BUILD 0' <<<"$exact_summary"; then
  echo "  PASS  exact      $exact_summary"
else
  echo "  FAIL  exact      ${exact_summary:-<no summary — inventory/harness error>}"
  grep -E '^\[[0-9]+/[0-9]+\] (DIFF|DEFER|HARNESS|MISSING_DEPENDENCY|UNSUPPORTED_BUILD)' <<<"$exact_out" \
    | head -5 | sed 's/^/          /'
  fail=1
fi

echo "-----------------------------------------------"
if [[ $fail -eq 0 ]]; then echo "GATE: GREEN"; else echo "GATE: FAILED"; fi
exit $fail
