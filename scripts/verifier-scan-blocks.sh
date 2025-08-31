#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/verifier-scan-blocks.sh <RPC_URL> <VERIFIER_PROGRAM_ID> [SLOTS]
# Scans recent slots using getBlock and reports how many successful txs
# that invoked the verifier have returnData produced by the verifier.

RPC_URL=${1:-}
VERIFIER=${2:-}
SLOTS=${3:-200}

if [[ -z "$RPC_URL" || -z "$VERIFIER" ]]; then
  echo "Usage: $0 <RPC_URL> <VERIFIER_PROGRAM_ID> [SLOTS]" >&2
  exit 1
fi

echo "RPC_URL=$RPC_URL"
echo "VERIFIER=$VERIFIER"
echo "SLOTS=$SLOTS"

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

# Get current slot
CURRENT=$(curl -sS -X POST -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getSlot"}' "$RPC_URL" | jq -r '.result')
if [[ -z "$CURRENT" || "$CURRENT" == "null" ]]; then
  echo "Failed to fetch current slot" >&2
  exit 1
fi

START=$((CURRENT - SLOTS))
if (( START < 0 )); then START=0; fi

total=0
with_rd=0
without_rd=0

for ((slot=CURRENT; slot>=START; slot--)); do
  resp=$(curl -sS -X POST -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getBlock","params":['"$slot"', {"encoding":"jsonParsed","maxSupportedTransactionVersion":0,"transactionDetails":"full","rewards":false}]}' \
    "$RPC_URL")

  # Skip missing slots
  if [[ $(echo "$resp" | jq -r '.result | type') != "object" ]]; then
    continue
  fi

  txlen=$(echo "$resp" | jq -r '.result.transactions | length')
  if [[ "$txlen" == "null" || "$txlen" == "0" ]]; then
    continue
  fi

  for ((i=0; i<txlen; i++)); do
    tx=$(echo "$resp" | jq ".result.transactions[$i]")
    status=$(echo "$tx" | jq -r '.meta.status // null | if type=="object" and has("Ok") then "ok" else "err" end')
    [[ "$status" == "ok" ]] || continue

    # Determine if this tx invoked the verifier program
    invoked=$(echo "$tx" | jq -r --arg v "$VERIFIER" '
      .transaction.message as $m |
      if ($m | has("parsed")) then
        ($m.parsed.instructions // [] | map(.programId == $v) | any)
      else
        ($m.instructions // [] | map(.programId == $v) | any)
      end')
    [[ "$invoked" == "true" ]] || continue

    total=$((total+1))
    producer=$(echo "$tx" | jq -r '.meta.returnData // empty | .programId // empty')
    if [[ -n "$producer" && "$producer" == "$VERIFIER" ]]; then
      with_rd=$((with_rd+1))
    else
      without_rd=$((without_rd+1))
    fi
  done
done

echo "scanned_total_invocations=$total"
echo "success_with_verifier_return_data=$with_rd"
echo "success_without_verifier_return_data_or_other_producer=$without_rd"
if [[ $total -gt 0 ]]; then
  pct=$(awk -v a=$with_rd -v b=$total 'BEGIN{printf "%.2f", (a*100.0)/b}')
  echo "verifier_last_writer_rate_pct=$pct"
fi

