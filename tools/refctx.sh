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
# Treat Shift-JIS source as bytes in the shell transforms. Locale-aware macOS
# sed otherwise rejects valid project files with "illegal byte sequence" before
# either compiler runs, falsely classifying them as HARNESS failures.
export LC_ALL=C

project="${1:?usage: refctx.sh <project_dir> <src/file.c> [build] [cflags…]}"
src="${2:?need a source file relative to the project}"
build="${3:-GC/2.6}"
[[ "$build" == */* ]] || build="GC/$build"
shift $(( $# < 3 ? $# : 3 ))
extra=("$@")

FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
wibo="$FFCC/build/tools/wibo"
sjis="$FFCC/build/tools/sjiswrap.exe"
compiler="$FFCC/build/compilers/$build/mwcceppc.exe"
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
here="$(cd "$(dirname "$0")/.." && pwd)"
ours="${MWCC_BIN:-$here/target/release/mwcc}"

project="$(cd "$project" && pwd)"
dir="$(mktemp -d "${TMPDIR:-/tmp}/refctx.XXXXXX")"
# The input suffix selects mwcceppc's language. Flattening every TU to `ctx.c`
# silently forced `.cpp`/`.cp` reference sources into C mode, producing false
# HARNESS failures before our compiler ran. Both sides use the same synthetic
# basename so the FILE symbol remains byte-identical too.
case "$src" in
  *.cpp|*.cp|*.cxx|*.cc) ctx_name="ctx.cpp";;
  *)                     ctx_name="ctx.c";;
esac
# Only ever remove the mktemp scratch dir — guard against a clobbered $dir so the
# cleanup can never delete a real tree (a stray loop variable once aliased $dir).
if [[ "${REFCTX_KEEP:-0}" == 1 ]]; then
  trap 'echo "refctx scratch retained: $dir" >&2' EXIT
else
  trap 'case "$dir" in */refctx.??????) rm -rf "$dir";; esac' EXIT
fi

# Base flags shared by both compilers. A manifest runner can pass the project's
# exact resolved flag vector as ordinary trailing arguments and explicitly
# suppress this historical marioparty4 fallback with REFCTX_EMPTY_BASE=1.
if [[ "${REFCTX_EMPTY_BASE:-0}" == 1 ]]; then
  base=()
elif [[ -n "${REFCTX_BASE:-}" ]]; then
  read -r -a base <<< "$REFCTX_BASE"
else
  base=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware \
    -Cpp_exceptions off -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off \
    -fp_contract on -str reuse)
fi

# 1. Inline includes into a self-contained file (run from the project root). The
#    include search path comes from (in priority order): an explicit REFCTX_INCLUDES
#    env; else the project's own `compile_flags.txt` (`-I`/`-isystem` roots — the real
#    build's include layout); else `include`. Auto-discovery matters: a project whose
#    headers live outside `include` (e.g. super_smash_brothers_melee under `src`,
#    `extern/dolphin/include`) would otherwise fail to inline `dolphin.h` and produce
#    a 36-line STUB (implicit-declaration artifact) instead of the real ~6000-line TU —
#    yielding false BYTE results. Discovering the real roots feeds decompctx the true
#    compilation unit.
all_flags=(${base[@]+"${base[@]}"} ${extra[@]+"${extra[@]}"})
compiler_flags=()
discovered_includes=()
for ((flag_index=0; flag_index<${#all_flags[@]}; flag_index++)); do
  flag="${all_flags[$flag_index]}"
  case "$flag" in
    -i|-I|-ir|-isystem)
      next_index=$((flag_index+1))
      if ((next_index < ${#all_flags[@]})); then
        discovered_includes+=("${all_flags[$next_index]}")
        flag_index=$next_index
      fi
      ;;
    -I+*) discovered_includes+=("${flag#-I+}");;
    -I?*) discovered_includes+=("${flag#-I}");;
    *) compiler_flags+=("$flag");;
  esac
done

if [[ -n "${REFCTX_INCLUDES:-}" ]]; then
  read -r -a include_dirs <<< "$REFCTX_INCLUDES"
else
  # Prefer the actual compile command's access paths. Project configure files
  # spell these as `-i dir`, `-I dir`, `-ir dir`, or their joined variants.
  include_dirs=(${discovered_includes[@]+"${discovered_includes[@]}"})
fi

if [[ ${#include_dirs[@]} -eq 0 && -f "$project/compile_flags.txt" ]]; then
  include_dirs=()
  while IFS= read -r inc; do
    [[ -d "$project/$inc" ]] && include_dirs+=("$inc")
  done < <(sed -nE 's/^-I//p; s/^-isystem//p' "$project/compile_flags.txt")
  [[ ${#include_dirs[@]} -gt 0 ]] || include_dirs=(include)
elif [[ ${#include_dirs[@]} -eq 0 ]]; then
  include_dirs=(include)
  # MSL system headers may live in a SUBROOT (pikmin: include/stl) or entirely
  # outside include/ (BfBB: src/PowerPC_EABI_Support/include) — add any
  # directory holding stddef.h so `#include <stddef.h>` resolves. Skip vendored
  # originals and build outputs.
  # fdlibm.h marks the MSL math include root — wind_waker keeps it in
  # Math/Include with none of the other markers, leaving __LO/__HI
  # unresolved (the real compiler then fails on `__LO(w) = 0`).
  while IFS= read -r sysroot; do
    rel="${sysroot#"$project"/}"
    [[ "$rel" == include ]] || include_dirs+=("$rel")
  done < <(find "$project" -maxdepth 8 \( -name stddef.h -o -name errno.h -o -name __va_arg.h -o -name fdlibm.h \) \
             -not -path "*/orig/*" -not -path "*/build/*" -not -path "*/tools/*" 2>/dev/null \
           | xargs -n1 dirname | sort -u)
fi
include_flags=()
# NB: not `dir` — that variable holds the mktemp scratch dir the EXIT trap removes.
for inc in "${include_dirs[@]}"; do include_flags+=(-I "$inc"); done
decompctx_log="$dir/decompctx.log"
( cd "$project" && python3 "$here/tools/decompctx_runner.py" tools/decompctx.py \
    "$src" "${include_flags[@]}" -o "$dir/$ctx_name" ) \
  >"$decompctx_log" 2>&1 || { echo "decompctx failed for $src"; exit 1; }

# decompctx is intentionally permissive: when an include is absent it logs the
# path, emits an empty include region, and exits successfully. Keep those paths
# so a later reference-compiler rejection can be distinguished from an actual
# harness/compiler error. A basename available elsewhere in the project (MSL's
# stddef.h roots, for example) is not called an absent dependency here.
missing_dependencies=()
while IFS= read -r missing; do
  [[ -n "$missing" ]] || continue
  normalized_missing="${missing//\\//}"
  if ! find "$project" -type f -path "*/$normalized_missing" -print -quit 2>/dev/null | grep -q .; then
    missing_dependencies+=("$normalized_missing")
  fi
done < <(sed -n 's/^Failed to locate //p' "$decompctx_log" | sort -u)

# 2. Preprocess the self-contained file to a clean .i for our mwcc (which does not
#    preprocess). mwcceppc drops language-changing pragmas from `-E` output, so
#    preserve the subset our parser models as inert declaration sentinels and
#    restore them afterward at their original positions. This matters for MSL
#    headers whose inline-local symbols mangle only inside `cplusplus` scopes.
preprocess_name="preprocess_$ctx_name"
sed -E \
  -e 's/^[[:space:]]*#pragma[[:space:]]+push[[:space:]]*$/extern int __mwcc_refctx_pragma_push;/' \
  -e 's/^[[:space:]]*#pragma[[:space:]]+pop[[:space:]]*$/extern int __mwcc_refctx_pragma_pop;/' \
  -e 's/^[[:space:]]*#pragma[[:space:]]+cplusplus[[:space:]]+on[[:space:]]*$/extern int __mwcc_refctx_pragma_cplusplus_on;/' \
  -e 's/^[[:space:]]*#pragma[[:space:]]+cplusplus[[:space:]]+off[[:space:]]*$/extern int __mwcc_refctx_pragma_cplusplus_off;/' \
  -e 's/^[[:space:]]*#pragma[[:space:]]+cplusplus[[:space:]]+reset[[:space:]]*$/extern int __mwcc_refctx_pragma_cplusplus_reset;/' \
  "$dir/$ctx_name" > "$dir/$preprocess_name"
( cd "$dir" && "$wibo" "$sjis" "$compiler" ${compiler_flags[@]+"${compiler_flags[@]}"} -E "$preprocess_name" -o ctx.marked.i ) 2>/dev/null
sed -E \
  -e 's/^[[:space:]]*extern int __mwcc_refctx_pragma_push;[[:space:]]*$/#pragma push/' \
  -e 's/^[[:space:]]*extern int __mwcc_refctx_pragma_pop;[[:space:]]*$/#pragma pop/' \
  -e 's/^[[:space:]]*extern int __mwcc_refctx_pragma_cplusplus_on;[[:space:]]*$/#pragma cplusplus on/' \
  -e 's/^[[:space:]]*extern int __mwcc_refctx_pragma_cplusplus_off;[[:space:]]*$/#pragma cplusplus off/' \
  -e 's/^[[:space:]]*extern int __mwcc_refctx_pragma_cplusplus_reset;[[:space:]]*$/#pragma cplusplus reset/' \
  "$dir/ctx.marked.i" > "$dir/ctx.i"
if [[ ! -s "$dir/ctx.i" ]]; then
  # An effectively EMPTY TU (sunshine's exponentialsf.c is a single
  # newline): mwcc -E emits nothing, but both compilers produce the
  # trivial object — continue with an empty .i. Anything with real
  # content that still failed -E is a genuine harness error.
  if grep -q '[^[:space:]]' "$dir/$ctx_name"; then
    echo "preprocess produced no .i"; exit 1
  fi
  : > "$dir/ctx.i"
fi

# 3a. Reference object from the real compiler (from the self-contained context).
if ! reference_output="$(
  cd "$dir" && "$wibo" "$sjis" "$compiler" \
    ${compiler_flags[@]+"${compiler_flags[@]}"} -c "$ctx_name" -o ref.o 2>&1
)"; then
  if [[ ${#missing_dependencies[@]} -gt 0 ]]; then
    echo "MISSING_DEPENDENCY  $src — ${missing_dependencies[0]}"
    exit 0
  fi
  printf '%s\n' "$reference_output" >&2
  exit 1
fi
[[ -f "$dir/ref.o" ]] || { echo "real mwcc rejected $ctx_name"; exit 1; }

# 3b. Our object. Preserve that synthetic basename so our FILE symbol matches.
#     Pass the same flags the real compiler got — our mwcc models the ones it knows
#     and ignores the rest.
mkdir -p "$dir/ours" && cp "$dir/ctx.i" "$dir/ours/$ctx_name"
if ! "$ours" --build "$build" ${compiler_flags[@]+"${compiler_flags[@]}"} -c "$dir/ours/$ctx_name" -o "$dir/our.o" 2>"$dir/oerr"; then
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
