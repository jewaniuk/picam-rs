#!/usr/bin/env bash
# progress.sh - uses `dead_code` warnings on unused items in `sys.rs` as a
# rough progress signal for how much of the vendor C API has been wired up
# to something in the safe wrapper layer so far. Run from anywhere in the
# repo; requires `jq` (`brew install jq` if you don't have it).
set -euo pipefail
cd "$(dirname "$0")"   # adjust if you save this somewhere other than the crate root

json=$(cargo check --message-format=json 2>/dev/null)

messages=$(echo "$json" | jq -r '
  select(.reason == "compiler-message")
  | .message
  | select(.code.code == "dead_code")
  | .message
')

echo "=== dead_code warnings by item kind ==="
echo "$messages" | sed -E 's/^([A-Za-z ]+) `.*/\1/' | sort | uniq -c | sort -rn
echo

total_warnings=$(echo "$messages" | grep -c . || true)
echo "total dead_code warnings: $total_warnings"
echo

# cross-reference against the real header count, independent of whatever
# bindgen actually generated - if wrapped + dead != total, something is
# either missing from the allowlist or double-counted
total_functions=$(grep -hoE 'PICAM_API\s+[A-Za-z_][A-Za-z0-9_]*\s*\(' vendor/*.h | wc -l | tr -d ' ')
dead_functions=$(echo "$messages" | grep -c '^function `' || true)
wrapped=$((total_functions - dead_functions))
pct=$((100 * wrapped / total_functions))
echo "functions wrapped: $wrapped / $total_functions ($pct%)"
