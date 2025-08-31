#!/usr/bin/env python3
import os
import sys
import time
import json
import base64
import requests

VERIFIER_PROGRAM = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c"

RPC_URL = os.environ.get("SOLANA_RPC", "https://api.mainnet-beta.solana.com")


def rpc(method, params):
    body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    r = requests.post(RPC_URL, json=body, timeout=30)
    r.raise_for_status()
    j = r.json()
    if "error" in j:
        raise RuntimeError(j["error"]) 
    return j["result"]


def fetch_recent_signatures(limit=1000):
    # getSignaturesForAddress newest-first; we page using before
    signatures = []
    before = None
    while len(signatures) < limit:
        batch = rpc("getSignaturesForAddress", [VERIFIER_PROGRAM, {"limit": 1000, "before": before}])
        if not batch:
            break
        signatures.extend(batch)
        before = batch[-1]["signature"]
        if len(signatures) >= limit:
            break
    return signatures[:limit]


def analyze(limit=500):
    sigs = fetch_recent_signatures(limit)
    ok = 0
    no_return = 0
    last_writer_not_verifier = 0
    examples = {"no_return": [], "not_verifier": []}

    for s in sigs:
        sig = s["signature"]
        tx = rpc("getTransaction", [sig, {"encoding": "json", "maxSupportedTransactionVersion": 0}])
        if not tx or not tx.get("meta"):
            continue
        meta = tx["meta"]
        if meta.get("err"):
            continue  # failure; we only care about success
        ok += 1
        rdata = meta.get("returnData")
        if not rdata:
            no_return += 1
            if len(examples["no_return"]) < 5:
                examples["no_return"].append(sig)
            continue
        pid = rdata.get("programId")
        if pid != VERIFIER_PROGRAM:
            last_writer_not_verifier += 1
            if len(examples["not_verifier"]) < 5:
                examples["not_verifier"].append({"sig": sig, "programId": pid})

    print(json.dumps({
        "scanned_success": ok,
        "success_no_return_data": no_return,
        "success_last_writer_not_verifier": last_writer_not_verifier,
        "examples": examples,
        "rpc": RPC_URL,
    }, indent=2))


if __name__ == "__main__":
    limit = int(sys.argv[1]) if len(sys.argv) > 1 else 500
    analyze(limit)

