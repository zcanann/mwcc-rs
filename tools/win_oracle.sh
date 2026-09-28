#!/usr/bin/env bash
# Run the canary oracle natively on Windows: sjiswrap stands in for wibo.
# usage: tools/win_oracle.sh <build> [filter]   (e.g. 2.6, 1.2.5n, Wii/1.0)
here="$(cd "$(dirname "$0")/.." && pwd)"
FFCC="${FFCC:-$(cd "$here/../FFCC-Decomp" && pwd -W)}"
export FFCC MWCC_EXPERIMENTAL_BUILDS=1
export REFCTX_WIBO="${REFCTX_WIBO:-$FFCC/build/tools/sjiswrap.exe}" REFCTX_SJISWRAP=""
export REFCTX_OBJDUMP="${REFCTX_OBJDUMP:-$FFCC/build/binutils/powerpc-eabi-objdump.exe}"
build="$1"; [[ "$build" == */* ]] || build="GC/$build"
if [[ -z "${MWCC_ORACLE_COMPILER:-}" ]]; then
  export MWCC_ORACLE_COMPILER="$(python3 -c "import sys;sys.path.insert(0,r'$(cd "$here" && pwd -W)/tools');import win_parity as w;print(w.reference_compiler('$build') or '')")"
fi
[[ -n "${2:-}" ]] && export MWCC_ORACLE_FILTER="$2"
exec "$here/target/release/mwcc-oracle" "$build"
