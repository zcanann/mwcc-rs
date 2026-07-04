#!/usr/bin/env bash
# Capture a reference TU for the exact-match pipeline: build the real object
# via refctx's include discovery, then emit <out>/real.o, real.dis, pool.txt.
#
# Usage: tools/capture.sh <project_dir> <src-relative> <out_dir> [version]
set -euo pipefail
project="${1:?usage: capture.sh <project> <src> <out> [version]}"
src="${2:?need a source path}"
out="${3:?need an output dir}"
version="${4:-2.6}"

FFCC="${FFCC:-/Users/zcanann/Documents/projects/FFCC-Decomp}"
wibo="$FFCC/build/tools/wibo"
sjis="$FFCC/build/tools/sjiswrap.exe"
compiler="$FFCC/build/compilers/GC/$version/mwcceppc.exe"
objdump="$FFCC/build/binutils/powerpc-eabi-objdump"
mkdir -p "$out"

# Include discovery MUST match tools/refctx.sh exactly (the gate of record) —
# fire 474: melee's compile_flags.txt adds extern/dolphin/include, changing the
# skipped-inline FINGERPRINT vs the marker-only discovery.
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
  while IFS= read -r sysroot; do
    rel="${sysroot#"$project"/}"
    [[ "$rel" == include ]] || include_dirs+=("$rel")
  done < <(find "$project" -maxdepth 8 \( -name stddef.h -o -name errno.h -o -name __va_arg.h -o -name fdlibm.h \) \
             -not -path "*/orig/*" -not -path "*/build/*" -not -path "*/tools/*" 2>/dev/null \
           | xargs -n1 dirname | sort -u)
fi
include_flags=()
for inc in "${include_dirs[@]}"; do include_flags+=(-I "$inc"); done
( cd "$project" && python3 tools/decompctx.py "$src" "${include_flags[@]}" -o "$out/ctx.c" ) >/dev/null 2>&1 \
  || { echo "decompctx failed"; exit 1; }

base=(-nodefaults -proc gekko -align powerpc -enum int -fp hardware \
  -Cpp_exceptions off -O4,p -inline auto -maxerrors 1 -nosyspath -RTTI off \
  -fp_contract on -str reuse)
( cd "$out" && "$wibo" "$sjis" "$compiler" "${base[@]}" -c ctx.c -o real.o ) 2>/dev/null
[[ -f "$out/real.o" ]] || { echo "real mwcc rejected ctx.c"; exit 1; }
"$objdump" -dr "$out/real.o" > "$out/real.dis"

python3 - "$objdump" "$out/real.o" > "$out/pool.txt" <<'PYEOF'
import subprocess, re, sys
objdump, obj = sys.argv[1], sys.argv[2]
o = subprocess.run([objdump, "-t", obj], capture_output=True, text=True).stdout
syms = re.findall(r'^([0-9a-f]+) l\s+O \.sdata2\s+[0-9a-f]+ (@\d+)$', o, re.M)
d = subprocess.run([objdump, "-s", "-j", ".sdata2", obj], capture_output=True, text=True).stdout
data = "".join("".join(l.split()[1:5]) for l in d.splitlines() if re.match(r'^ [0-9a-f]{4} ', l))
for off, name in syms:
    print(name, data[int(off,16)*2:int(off,16)*2+16])
PYEOF
echo "captured: $(grep -cE '^\s*[0-9a-f]+:\s+([0-9a-f]{2} ){4}' "$out/real.dis") instructions, $(wc -l < "$out/pool.txt" | tr -d ' ') pools"
