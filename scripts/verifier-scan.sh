#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/verifier-scan.sh <RPC_URL> <VERIFIER_PROGRAM_ID> [LIMIT]
# Scans recent transactions that mention the verifier program and reports
# how many successful txs have returnData set by the verifier, and how many do not.

RPC_URL=${1:-}
VERIFIER=${2:-}
LIMIT=${3:-1000}

if [[ -z "$RPC_URL" || -z "$VERIFIER" ]]; then
  echo "Usage: $0 <RPC_URL> <VERIFIER_PROGRAM_ID> [LIMIT]" >&2
  exit 1
fi

echo "RPC_URL=$RPC_URL"
echo "VERIFIER=$VERIFIER"
echo "LIMIT=$LIMIT"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

# Fetch recent signatures referencing the verifier program id
curl -sS -X POST -H 'Content-Type: application/json' \
  --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getSignaturesForAddress\",\"params\":[\"$VERIFIER\",{\"limit\":$LIMIT}]}" \
  "$RPC_URL" | jq -r '.result[].signature' > "$tmpdir/signatures.txt"

total=0
with_rd=0
without_rd=0

while IFS= read -r sig; do
  # Fetch full transaction with parsed message
  resp=$(curl -sS -X POST -H 'Content-Type: application/json' \
    --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTransaction\",\"params\":[\"$sig\",{\"encoding\":\"jsonParsed\",\"maxSupportedTransactionVersion\":0}]}" \
    "$RPC_URL")

  status=$(echo "$resp" | jq -r '.result.meta.status // null | if type=="object" and has("Ok") then "ok" else "err" end')
  if [[ "$status" != "ok" ]]; then
    continue
  fi

  # Verify the tx actually invoked the verifier program in some instruction
  invoked=$(echo "$resp" | jq -r --arg v "$VERIFIER" '
    .result.transaction.message as $m |
    if ($m | has("parsed")) then
      ($m.parsed.instructions[]? | .programId == $v) // false
    else
      ($m.instructions[]? | .programId == $v) // false
    end' | grep -q true && echo yes || echo no)
  if [[ "$invoked" != "yes" ]]; then
    continue
  fi

  total=$((total+1))
  producer=$(echo "$resp" | jq -r '.result.meta.returnData // empty | .programId // empty')
  if [[ -n "$producer" && "$producer" == "$VERIFIER" ]]; then
    with_rd=$((with_rd+1))
  else
    without_rd=$((without_rd+1))
  fi
done < "$tmpdir/signatures.txt"

echo "scanned_total_invocations=$total"
echo "success_with_verifier_return_data=$with_rd"
echo "success_without_verifier_return_data_or_other_producer=$without_rd"

if [[ $total -gt 0 ]]; then
  pct=$(awk -v a=$with_rd -v b=$total 'BEGIN{printf "%.2f", (a*100.0)/b}')
  echo "verifier_last_writer_rate_pct=$pct"
fi

