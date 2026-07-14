#!/usr/bin/env bash
# Codegen-defer REASON histogram for a project's src/ — the prioritized worklist
# for advancing the GENERAL compiler (each frequent reason is one feature that
# flips many TUs from DEFER to PERFECT). Complements parity.sh (which tallies
# BYTE/DIFF/DEFER counts but discards the per-TU reason).
#
# Usage: tools/defer_histogram.sh <project_dir> [version]
here="$(cd "$(dirname "$0")" && pwd)"
project="${1:?usage: defer_histogram.sh <project> [version]}"
version="${2:-2.6}"
tmp="$(mktemp)"
total=0
while IFS= read -r src; do
  rel="${src#"$project"/}"
  total=$((total+1))
  line="$("$here/refctx.sh" "$project" "$rel" "$version" 2>&1 | head -1 || true)"
  case "$line" in
    DEFER*) printf '%s\n' "${line#*— }" >> "$tmp";;   # keep the reason text
  esac
  printf '\r  swept %d TUs' "$total" >&2
done < <(find "$project/src" -name '*.c' | sort)
echo >&2; echo
echo "════ CODEGEN-DEFER REASONS: $(basename "$project") @ GC/$version ════"
# Normalize: strip trailing "(roadmap …)" specifics and per-TU identifiers so like
# reasons bucket together; show the most frequent first.
sed -E 's/ \(roadmap[^)]*\)//; s/'"'"'[^'"'"']*'"'"'/X/g; s/[0-9]+/N/g' "$tmp" \
  | sort | uniq -c | sort -rn | head -30
echo "── ($(wc -l < "$tmp" | tr -d ' ') defers over $total TUs) ──"
rm -f "$tmp"
