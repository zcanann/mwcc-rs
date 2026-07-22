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
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
here="$(cd "$(dirname "$0")/.." && pwd)"
ours="${MWCC_BIN:-$here/target/release/mwcc}"
code_metrics="$here/tools/object_code_metrics.py"
pch_scanner="$here/tools/refctx_pch.py"
pragma_bridge="$here/tools/refctx_pragmas.py"

project="$(cd "$project" && pwd)"
# Resolve the real compiler independently from the tools checkout. Some builds
# used by reference projects (notably GC/3.0a3p1) live in the compiler archive
# but not FFCC's curated compiler set. An explicit executable wins, followed by
# FFCC, caller-provided roots, and the archive beside reference_projects/.
compiler="${MWCC_REFERENCE_COMPILER:-$FFCC/build/compilers/$build/mwcceppc.exe}"
if [[ -z "${MWCC_REFERENCE_COMPILER:-}" && ! -f "$compiler" ]]; then
  compiler_roots=()
  if [[ -n "${REFCTX_COMPILER_ROOTS:-}" ]]; then
    IFS=: read -r -a compiler_roots <<< "$REFCTX_COMPILER_ROOTS"
  fi
  reference_parent="$(cd "$(dirname "$project")/.." && pwd)"
  compiler_roots+=("$reference_parent/misc/compilers_latest")
  for compiler_root in "${compiler_roots[@]}"; do
    candidate="$compiler_root/$build/mwcceppc.exe"
    if [[ -f "$candidate" ]]; then
      compiler="$candidate"
      break
    fi
  done
fi
if [[ ! -f "$compiler" ]]; then
  echo "PARITY_META oracle_direct=REFERENCE_COMPILER_MISSING"
  echo "MISSING_DEPENDENCY  $src — reference compiler $build not found"
  exit 0
fi
dir="$(mktemp -d "${TMPDIR:-/tmp}/refctx.XXXXXX")"
# The suffix selects mwcceppc's language and the basename becomes the ELF FILE
# symbol. Preserve the authoritative source basename in the self-contained
# fallback too: a generic `ctx.cpp` made otherwise-identical direct objects look
# different solely because their FILE symbols disagreed.
source_name="${src##*/}"
ctx_name="$source_name"
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

# Prefer the authoritative project input. Real MWCC can resolve most projects'
# exact access paths directly; preprocessing that source preserves conditional
# include order, macro state, and include guards that decompctx cannot model.
# Keep the original basename on our preprocessed copy so FILE symbols match.
mkdir -p "$dir/ours"
direct_ready=0
oracle_direct="REJECTED"
direct_reference_output=""
emit_oracle_meta() {
  echo "PARITY_META oracle_direct=$oracle_direct"
}
if direct_reference_output="$(
  cd "$project" && "$wibo" "$sjis" "$compiler" \
    ${all_flags[@]+"${all_flags[@]}"} -c "$src" -o "$dir/ref.o" 2>&1
)"; then
  oracle_direct="RUNNABLE"
  cp "$dir/ref.o" "$dir/ref.direct.o"
  # MWCC's `-E` deletes stateful pragmas. Preprocess a scratch copy whose
  # modeled pragmas are inert declarations, then restore them in the emitted
  # token stream. Mirroring the source directory preserves quoted sibling
  # includes without modifying the reference checkout.
  direct_source_dir="$dir/direct-source/$(dirname "$src")"
  mkdir -p "$direct_source_dir"
  for source_sibling in "$project/$(dirname "$src")"/*; do
    ln -s "$source_sibling" "$direct_source_dir/${source_sibling##*/}"
  done
  unlink "$direct_source_dir/$source_name"
  python3 "$pragma_bridge" mark "$project/$src" "$direct_source_dir/$source_name"
  direct_preprocess_ok=0
  if direct_preprocess_output="$(
    cd "$project" && "$wibo" "$sjis" "$compiler" \
      ${all_flags[@]+"${all_flags[@]}"} -pragma "line_prepdump on" \
      -E "$direct_source_dir/$source_name" -o "$dir/ours/$source_name.marked" 2>&1
  )"; then
    direct_preprocess_ok=1
  # The 2.3.3 standalone preprocessor rejects C++'s `or` alternative token in
  # directive expressions even though the integrated compile accepts it.  A
  # preprocessing-only macro is semantically identical and preserves the
  # original include traversal, avoiding a synthetic decompctx comparison for
  # Pikmin's shared DebugLog.h.
  elif grep -Eq '(^|[[:space:]])or([[:space:]]|$)' <<<"$direct_preprocess_output" \
      && grep -q 'expression syntax error' <<<"$direct_preprocess_output" \
      && direct_preprocess_output="$(
        cd "$project" && "$wibo" "$sjis" "$compiler" \
          ${all_flags[@]+"${all_flags[@]}"} "-Dor=||" \
          -pragma "line_prepdump on" -E "$direct_source_dir/$source_name" \
          -o "$dir/ours/$source_name.marked" 2>&1
      )"; then
    direct_preprocess_ok=1
  fi
  if [[ $direct_preprocess_ok -eq 1 ]]; then
    # MWCC emits no preprocessed file for an empty translation unit.
    [[ -f "$dir/ours/$source_name.marked" ]] || : > "$dir/ours/$source_name.marked"
    python3 "$pragma_bridge" restore "$dir/ours/$source_name.marked" \
      "$dir/ours/$source_name"
    direct_ready=1
    ctx_name="$source_name"
  fi
fi
# Emit the direct probe immediately so timeouts and every early harness exit
# retain provenance. A successful generated-PCH retry emits an updated value
# later; the machine-readable parser intentionally keeps the last value.
emit_oracle_meta

if [[ $direct_ready -eq 0 ]]; then
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

# Some reconstructed headers use C++ alternative tokens in preprocessor
# expressions (`#elif A or B`). MWCC accepts `or` in language expressions but
# its legacy preprocessor does not. Normalize only directive lines in the
# synthetic context, preserving line count and meaning for both compilers.
sed -E '/^[[:space:]]*#/ s/[[:space:]]+or[[:space:]]+/ || /g' \
  "$dir/$ctx_name" > "$dir/ctx.normalized"
mv "$dir/ctx.normalized" "$dir/$ctx_name"

# decompctx is intentionally permissive: when an include is absent it logs the
# path, emits an empty include region, and exits successfully. Keep those paths
# so a later reference-compiler rejection can be distinguished from an actual
# harness/compiler error. A basename available elsewhere in the project (MSL's
# stddef.h roots, for example) is not called an absent dependency here.
missing_dependencies=()
missing_precompiled_headers=()
# decompctx_runner expands a missing generated `.mch` from its textual `.pch`
# and records the original include in a source marker rather than its warning
# log. Recover those generated dependencies from the self-contained context.
while IFS= read -r generated_mch; do
  [[ -n "$generated_mch" ]] && missing_precompiled_headers+=("$generated_mch")
done < <(python3 "$pch_scanner" "$dir/$ctx_name")
while IFS= read -r missing; do
  [[ -n "$missing" ]] || continue
  normalized_missing="${missing//\\//}"
  # Project build graphs generate `.mch` precompiled headers from a same-path
  # `.pch` source (for example Twilight Princess's d/dolzel.{mch,pch}). A clean
  # checkout intentionally lacks the binary `.mch`; decompctx expands the `.pch`
  # source into the self-contained fallback, so this is not a missing source
  # dependency. Keep genuine orphaned `.mch` paths classified as missing.
  if [[ "$normalized_missing" == *.mch ]]; then
    pch_source="${normalized_missing%.mch}.pch"
    if find "$project" -type f -path "*/$pch_source" -print -quit 2>/dev/null | grep -q .; then
      missing_precompiled_headers+=("$normalized_missing")
      continue
    fi
  fi
  if ! find "$project" -type f -path "*/$normalized_missing" -print -quit 2>/dev/null | grep -q .; then
    missing_dependencies+=("$normalized_missing")
  fi
done < <(sed -n 's/^Failed to locate //p' "$decompctx_log" | sort -u)

# Clean decomp checkouts intentionally omit generated MWCC precompiled headers.
# An integrated compile can then continue after a missing `.mch` without its
# declarations and fail later with a misleading syntax/type error. Recreate
# each required PCH in scratch and retry the authoritative source compile. The
# source project remains untouched, and the row's exact compiler/flags produce
# the same input the normal Ninja build would have generated.
if [[ "$oracle_direct" != "RUNNABLE" && ${#missing_precompiled_headers[@]} -gt 0 ]]; then
  pch_root="$dir/generated-pch"
  pch_ready=1
  for missing_pch in "${missing_precompiled_headers[@]}"; do
    case "$missing_pch" in
      /*|*../*|../*) pch_ready=0; break;;
    esac
    pch_source="${missing_pch%.mch}.pch"
    pch_source_path="$(find "$project" -type f -path "*/$pch_source" -print -quit 2>/dev/null)"
    if [[ -z "$pch_source_path" ]]; then
      pch_ready=0
      break
    fi
    pch_output_dir="$pch_root/$(dirname "$missing_pch")"
    mkdir -p "$pch_output_dir"
    if ! (
      cd "$project" && "$wibo" "$sjis" "$compiler" \
        ${all_flags[@]+"${all_flags[@]}"} -lang=c++ -c "$pch_source_path" \
        -o "$pch_output_dir" -precompile "$(basename "$missing_pch")"
    ) >"$dir/pch.log" 2>&1; then
      pch_ready=0
      break
    fi
  done
  if [[ $pch_ready -eq 1 ]]; then
    if direct_reference_output="$(
      cd "$project" && "$wibo" "$sjis" "$compiler" -i "$pch_root" \
        ${all_flags[@]+"${all_flags[@]}"} -c "$src" -o "$dir/ref.o" 2>&1
    )"; then
      oracle_direct="RUNNABLE"
      cp "$dir/ref.o" "$dir/ref.direct.o"
      # Keep the direct object as the authoritative oracle, but do NOT use `-E`
      # through the generated PCH as our compiler input.  MWCC's preprocessed
      # output contains only tokens written after the PCH include; declarations,
      # inline definitions, and retained static data restored from the PCH are
      # absent even though they participate in the direct object.  Comparing that
      # declaration-free tail against the PCH-backed object is not an A/B test of
      # equivalent inputs.  Leave `direct_ready` false so the fallback below feeds
      # mwcc-rs the textual `.pch` expansion already produced by decompctx.  An
      # exact result is still authoritative because `ref.o` remains direct; a
      # synthetic-input defer/diff is correctly reported as measurement-unknown.
    fi
  fi
fi

if [[ $direct_ready -eq 0 ]]; then
# A direct compile gives the authoritative missing-file diagnostic. decompctx
# may also visit missing includes inside inactive conditionals, so its log alone
# is insufficient to classify a dependency as required.
if [[ ${#missing_dependencies[@]} -gt 0 ]] \
  && grep -Eqi 'cannot be opened|can.t be opened|file not found|no such file' \
       <<<"$direct_reference_output"; then
  emit_oracle_meta
  echo "MISSING_DEPENDENCY  $src — ${missing_dependencies[0]}"
  exit 0
fi
if grep -q 'Unknown option' <<<"$direct_reference_output"; then
  invalid_detail="$(grep -m1 'Unknown option' <<<"$direct_reference_output" | sed 's/^[#[:space:]]*//')"
  emit_oracle_meta
  echo "INVALID_CONFIGURATION  $src — $invalid_detail"
  exit 0
fi

# 2. Preprocess the self-contained file to a clean .i for our mwcc (which does not
#    preprocess). mwcceppc drops language-changing pragmas from `-E` output, so
#    preserve the subset our parser models as inert declaration sentinels and
#    restore them afterward at their original positions. This matters for MSL
#    headers whose inline-local symbols mangle only inside `cplusplus` scopes.
preprocess_name="preprocess_$ctx_name"
python3 "$pragma_bridge" mark "$dir/$ctx_name" "$dir/$preprocess_name"
# decompctx_runner populates generated `.mch` include arms from their textual
# `.pch` sources, so the real preprocessor can retain its normal `__MWERKS__`
# branch selection while operating on a clean checkout.
( cd "$dir" && "$wibo" "$sjis" "$compiler" \
    ${compiler_flags[@]+"${compiler_flags[@]}"} -pragma "line_prepdump on" \
    -E "$preprocess_name" -o ctx.marked.i ) 2>/dev/null
python3 "$pragma_bridge" restore "$dir/ctx.marked.i" "$dir/ctx.i"
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

# Measure the actual project invocation before attempting the optional
# preprocessed-core comparison.  A generated PCH may make the authoritative
# source compile runnable while MWCC still rejects decompctx's textual bridge;
# that bridge failure must not erase an already measurable drop-in result.
measure_configured_source() {
  local configured_source="UNAVAILABLE"
  local -a configured_extra=()
  if [[ "$oracle_direct" == "RUNNABLE" ]]; then
    if [[ -n "${pch_root:-}" && -d "${pch_root:-}" ]]; then
      configured_extra=(-i "$pch_root")
    fi
    if (
      cd "$project" && "$ours" --build "$build" \
        ${configured_extra[@]+"${configured_extra[@]}"} \
        ${all_flags[@]+"${all_flags[@]}"} -c "$src" \
        -o "$dir/our.configured.o"
    ) >"$dir/our.configured.log" 2>&1; then
      if cmp -s "$dir/ref.direct.o" "$dir/our.configured.o"; then
        configured_source="BYTE"
      else
        configured_source="DIFF"
      fi
    else
      configured_source="DEFER"
    fi
    echo "PARITY_META configured_source=$configured_source"
  fi
}

# The initial direct probe may have been rejected before the generated-PCH
# retry made it runnable.  Persist the final oracle state before any optional
# bridge step can fail and exit early.
emit_oracle_meta
measure_configured_source

# 3a. Compile the compiler-core reference from the exact same bridge handed to
#     mwcc-rs. Preprocessing is not code-neutral under every flag: legacy `-sym
#     on` can retain a different local-variable frame when compiling the original
#     source. Comparing that object to a bridge-compiled candidate manufactures
#     backend differences. The original object remains authoritative for the
#     configured-source comparison above.
if [[ $direct_ready -eq 1 ]]; then
  if ! (
    cd "$dir/ours" && "$wibo" "$sjis" "$compiler" \
      ${compiler_flags[@]+"${compiler_flags[@]}"} -c "$ctx_name" \
      -o "$dir/ref.o"
  ) >"$dir/reference.bridge.log" 2>&1; then
    printf '%s\n' "$(<"$dir/reference.bridge.log")" >&2
    exit 1
  fi
else
  if ! reference_output="$(
    cd "$dir" && "$wibo" "$sjis" "$compiler" \
      ${compiler_flags[@]+"${compiler_flags[@]}"} -c "$ctx_name" -o ref.o 2>&1
  )"; then
    if [[ ${#missing_dependencies[@]} -gt 0 ]]; then
      emit_oracle_meta
      echo "MISSING_DEPENDENCY  $src — ${missing_dependencies[0]}"
      exit 0
    fi
    if grep -q 'Unknown option' <<<"$reference_output"; then
      invalid_detail="$(grep -m1 'Unknown option' <<<"$reference_output" | sed 's/^[#[:space:]]*//')"
      emit_oracle_meta
      echo "INVALID_CONFIGURATION  $src — $invalid_detail"
      exit 0
    fi
    printf '%s\n' "$reference_output" >&2
    exit 1
  fi
  [[ -f "$dir/ref.o" ]] || { echo "real mwcc rejected $ctx_name"; exit 1; }
fi
cp "$dir/ctx.i" "$dir/ours/$ctx_name"
fi
fi

# 3b. Our object. Preserve that synthetic basename so our FILE symbol matches.
#     Pass the same flags the real compiler got — our mwcc models the ones it knows
#     and ignores the rest.
emit_oracle_meta
if [[ $direct_ready -eq 1 ]]; then
  echo "PARITY_META comparison_input=DIRECT"
else
  echo "PARITY_META comparison_input=SYNTHETIC"
fi
if [[ $direct_ready -eq 1 ]]; then
  echo "PARITY_META reference_object=PREPROCESSED"
else
  echo "PARITY_META reference_object=SYNTHETIC"
fi
if ! "$ours" --build "$build" ${compiler_flags[@]+"${compiler_flags[@]}"} -c "$dir/ours/$ctx_name" -o "$dir/our.o" 2>"$dir/oerr"; then
  # Parser recovery notes can precede a much later terminal codegen diagnostic.
  # The final `mwcc:` line is the actionable blocker; reporting the first stderr
  # line made frontier buckets describe harmless header recovery instead.
  defer_detail="$(sed -n 's/^mwcc: //p' "$dir/oerr" | tail -1)"
  [[ -n "$defer_detail" ]] || defer_detail="$(tail -1 "$dir/oerr")"
  echo "DEFER  $src — $defer_detail"
  # Full-object parity still fails when debug emission is absent. For compiler-
  # core visibility, retry BOTH compilers with a final `-sym off`. Comparing our
  # projection to the debug-enabled reference object creates a false anonymous-
  # ordinal difference because MWCC debug bookkeeping advances the @N stream.
  # This same-flags projection is diagnostic only and never earns whole-object
  # BYTE credit.
  if [[ "$defer_detail" == debug-info:* && "${REFCTX_CODE_PROJECTION:-0}" == 1 ]]; then
    reference_projected=0
    if [[ $direct_ready -eq 1 ]]; then
      if (
        cd "$dir/ours" && "$wibo" "$sjis" "$compiler" \
          ${compiler_flags[@]+"${compiler_flags[@]}"} -sym off -c "$ctx_name" \
          -o "$dir/reference.projected.o"
      ) >"$dir/reference.projected.log" 2>&1; then
        reference_projected=1
      fi
    else
      if (
        cd "$dir" && "$wibo" "$sjis" "$compiler" \
          ${compiler_flags[@]+"${compiler_flags[@]}"} -sym off -c "$ctx_name" \
          -o reference.projected.o
      ) >"$dir/reference.projected.log" 2>&1; then
        reference_projected=1
      fi
    fi
    if [[ $reference_projected -eq 0 ]]; then
      projected_detail="$(sed 's/^mwcc: //' "$dir/reference.projected.log" | head -1)"
      echo "CODE DEFER — reference -sym off projection failed: $projected_detail"
    elif "$ours" --build "$build" ${compiler_flags[@]+"${compiler_flags[@]}"} -sym off \
        -c "$dir/ours/$ctx_name" -o "$dir/projected.o" 2>"$dir/projected.err"; then
      python3 "$code_metrics" "$objdump" "$dir/reference.projected.o" "$dir/projected.o" \
        --context "the same-flags -sym off projection"
    else
      projected_detail="$(sed 's/^mwcc: //' "$dir/projected.err" | head -1)"
      echo "CODE DEFER — $projected_detail"
    fi
  fi
  exit 0
fi

if cmp -s "$dir/ref.o" "$dir/our.o"; then
  echo "BYTE   $src — whole object byte-identical ✅"
  python3 "$code_metrics" "$objdump" "$dir/ref.o" "$dir/our.o"
else
  echo "DIFF   $src — objects differ"
  python3 "$code_metrics" "$objdump" "$dir/ref.o" "$dir/our.o"
fi
