#!/usr/bin/env bash
# Real reference-TU A/B harness.
#
# Compiles one source file from a decomp reference_project through both the real
# mwcceppc and our mwcc, and compares the objects — the path toward matching whole
# reference projects 1:1.
#
# Because our mwcc does NOT preprocess, the input is fully preprocessed first:
#   1. tools/decompctx.py inlines every #include into one self-contained file
#      (sidesteps wibo's inability to resolve -i access paths),
#   2. real mwcceppc -E expands the remaining macros/#defines to a clean .i,
#   3. both compilers consume that .i.
#
# Usage:
#   tools/refctx.sh <project_dir> <src/file.c> [version] [extra cflags…]
# Example:
#   tools/refctx.sh ../Metrowerks/reference_projects/marioparty4 \
#       src/odenotstub/odenotstub.c 2.6 -inline auto,deferred
#
# Runs wibo, so do NOT run it while a full oracle sweep is in progress.
set -euo pipefail

project="${1:?usage: refctx.sh <project_dir> <src/file.c> [version] [cflags…]}"
src="${2:?need a source file relative to the project}"
version="${3:-2.6}"
shift $(( $# < 3 ? $# : 3 ))
extra=("$@")

FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
wibo="$FFCC/build/tools/wibo"
sjis="$FFCC/build/tools/sjiswrap.exe"
compiler="$FFCC/build/compilers/GC/$version/mwcceppc.exe"
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
here="$(cd "$(dirname "$0")/.." && pwd)"
ours="$here/target/release/mwcc"

project="$(cd "$project" && pwd)"
dir="$(mktemp -d "${TMPDIR:-/tmp}/refctx.XXXXXX")"
# Only ever remove the mktemp scratch dir — guard against a clobbered $dir so the
# cleanup can never delete a real tree (a stray loop variable once aliased $dir).
trap 'case "$dir" in */refctx.??????) rm -rf "$dir";; esac' EXIT

# 1. Inline includes into a self-contained file (run from the project root). The
#    include search path comes from (in priority order): an explicit REFCTX_INCLUDES
#    env; else the project's own `compile_flags.txt` (`-I`/`-isystem` roots — the real
#    build's include layout); else `include`. Auto-discovery matters: a project whose
#    headers live outside `include` (e.g. super_smash_brothers_melee under `src`,
#    `extern/dolphin/include`) would otherwise fail to inline `dolphin.h` and produce
#    a 36-line STUB (implicit-declaration artifact) instead of the real ~6000-line TU —
#    yielding false BYTE results. Discovering the real roots feeds decompctx the true
#    compilation unit.
if [[ -n "${REFCTX_INCLUDES:-}" ]]; then
  read -r -a include_dirs <<< "$REFCTX_INCLUDES"
elif [[ -f "$project/compile_flags.txt" ]]; then
  include_dirs=()
  while IFS= read -r inc; do
    [[ -d "$project/$inc" ]] && include_dirs+=("$inc")
  done < <(sed -nE 's/^-I//p; s/^-isystem//p' "$project/compile_flags.txt")
  [[ ${#include_dirs[@]} -gt 0 ]] || include_dirs=(include)
else
  include_dirs=(include)
  # MSL system headers may live in a SUBROOT (pikmin: include/stl) or entirely
  # outside include/ (BfBB: src/PowerPC_EABI_Support/include) — add any
  # directory holding stddef.h so `#include <stddef.h>` resolves. Skip vendored
  # originals and build outputs.
  while IFS= read -r sysroot; do
    rel="${sysroot#"$project"/}"
    [[ "$rel" == include ]] || include_dirs+=("$rel")
  done < <(find "$project" -maxdepth 8 \( -name stddef.h -o -name errno.h -o -name __va_arg.h \) \
             -not -path "*/orig/*" -not -path "*/build/*" -not -path "*/tools/*" 2>/dev/null \
           | xargs -n1 dirname | sort -u)
fi
include_flags=()
# NB: not `dir` — that variable holds the mktemp scratch dir the EXIT trap removes.
for inc in "${include_dirs[@]}"; do include_flags+=(-I "$inc"); done
( cd "$project" && python3 tools/decompctx.py "$src" "${include_flags[@]}" -o "$dir/ctx.c" ) >/dev/null 2>&1 \
  || { echo "decompctx failed for $src"; exit 1; }

# Base flags shared by every reference TU; the caller adds the per-group extras.
# Default to marioparty4's set; a different project (different -O/-inline/-char)
# can replace it wholesale via REFCTX_BASE="flag1 flag2 …" in the environment.
if [[ -n "${REFCTX_BASE:-}" ]]; then
  read -r -a base <<< "$REFCTX_BASE"
else
  base=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware \
    -Cpp_exceptions off -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off \
    -fp_contract on -str reuse)
fi

# 2. Preprocess the self-contained file to a clean .i for our mwcc (which does not
#    preprocess). mwcceppc only accepts .c/.cpp, so the real compiler builds the
#    reference straight from ctx.c (preprocessing it internally) — identical input.
( cd "$dir" && "$wibo" "$sjis" "$compiler" "${base[@]}" ${extra[@]+"${extra[@]}"} -E ctx.c -o ctx.i ) 2>/dev/null
if [[ ! -s "$dir/ctx.i" ]]; then echo "preprocess produced no .i"; exit 1; fi

# 3a. Reference object from the real compiler (from the self-contained ctx.c).
( cd "$dir" && "$wibo" "$sjis" "$compiler" "${base[@]}" ${extra[@]+"${extra[@]}"} -c ctx.c -o ref.o ) 2>/dev/null
[[ -f "$dir/ref.o" ]] || { echo "real mwcc rejected ctx.c"; exit 1; }

# 3b. Our object. Feed the preprocessed text under the name ctx.c so our FILE
#     symbol matches the reference's (which compiled ctx.c). Pass the same flags
#     the real compiler got — our mwcc models the ones it knows and ignores the rest.
mkdir -p "$dir/ours" && cp "$dir/ctx.i" "$dir/ours/ctx.c"
if ! "$ours" --build "GC/$version" "${base[@]}" ${extra[@]+"${extra[@]}"} -c "$dir/ours/ctx.c" -o "$dir/our.o" 2>"$dir/oerr"; then
  echo "DEFER  $src — $(sed 's/^mwcc: //' "$dir/oerr" | head -1)"
  exit 0
fi

if cmp -s "$dir/ref.o" "$dir/our.o"; then
  echo "BYTE   $src — whole object byte-identical ✅"
else
  echo "DIFF   $src — objects differ; first .text diff:"
  diff <("$objdump" -dr "$dir/ref.o" | sed -n '/>:/,/^$/p') \
       <("$objdump" -dr "$dir/our.o" | sed -n '/>:/,/^$/p') | head -30
fi
