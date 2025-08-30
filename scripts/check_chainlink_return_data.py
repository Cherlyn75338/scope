#!/usr/bin/env python3
import json
import sys
import time
from typing import Any, Dict, List, Optional, Tuple
from urllib.request import Request, urlopen


RPC_URL = "https://api.mainnet-beta.solana.com"
VERIFIER_PROGRAM_ID = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c"


def rpc_call(method: str, params: Any, timeout: int = 20) -> Any:
    payload = json.dumps({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    }).encode("utf-8")
    req = Request(RPC_URL, payload, headers={"Content-Type": "application/json"})
    with urlopen(req, timeout=timeout) as resp:
        body = resp.read().decode("utf-8")
        data = json.loads(body)
        if "error" in data:
            raise RuntimeError(f"RPC error for {method}: {data['error']}")
        return data["result"]


def get_signatures_for_address(address: str, limit: int = 60) -> List[str]:
    res = rpc_call("getSignaturesForAddress", [address, {"limit": limit}])
    return [entry["signature"] for entry in res]


def get_transaction(signature: str) -> Optional[Dict[str, Any]]:
    try:
        return rpc_call(
            "getTransaction",
            [
                signature,
                {
                    "encoding": "json",
                    "maxSupportedTransactionVersion": 0,
                    "commitment": "confirmed",
                },
            ],
        )
    except Exception as e:
        print(f"warn: getTransaction failed for {signature}: {e}", file=sys.stderr)
        return None


def is_success(meta: Dict[str, Any]) -> bool:
    return meta is not None and meta.get("err") is None


def extract_top_level_instructions(tx: Dict[str, Any]) -> Tuple[List[Dict[str, Any]], List[str]]:
    message = tx.get("transaction", {}).get("message", {})
    account_keys = message.get("accountKeys", [])
    # accountKeys may be list of dicts in v0; normalize to base58 strings
    keys: List[str] = []
    for k in account_keys:
        if isinstance(k, str):
            keys.append(k)
        elif isinstance(k, dict) and "pubkey" in k:
            keys.append(k["pubkey"])
        else:
            # Fallback: try to stringify
            keys.append(str(k))
    instructions = message.get("instructions", [])
    return instructions, keys


def program_id_for_ix(ix: Dict[str, Any], keys: List[str]) -> Optional[str]:
    # For legacy/JSON encoding, programIdIndex is present
    if "programIdIndex" in ix:
        try:
            idx = int(ix["programIdIndex"])
            if 0 <= idx < len(keys):
                return keys[idx]
        except Exception:
            return None
    # Some responses may include programId directly
    if "programId" in ix:
        return ix["programId"]
    return None


def analyze_transactions(signatures: List[str]) -> None:
    stats = {
        "total": 0,
        "tx_success": 0,
        "verifier_invoked": 0,
        "single_ix_success": 0,
        "single_ix_success_return_program_match": 0,
        "single_ix_success_no_return_data": 0,
        "single_ix_success_return_program_mismatch": 0,
    }
    examples = {
        "no_return_data": [],
        "mismatch": [],
        "match": [],
    }

    for i, sig in enumerate(signatures):
        tx = get_transaction(sig)
        if not tx:
            continue
        stats["total"] += 1
        meta = tx.get("meta") or {}
        logs = meta.get("logMessages") or []
        success = is_success(meta)
        if success:
            stats["tx_success"] += 1

        instructions, keys = extract_top_level_instructions(tx)
        # Count how many top-level instructions target the verifier
        verifier_tl_ix_indices = [idx for idx, ix in enumerate(instructions) if program_id_for_ix(ix, keys) == VERIFIER_PROGRAM_ID]
        verifier_invoked = len(verifier_tl_ix_indices) > 0
        if verifier_invoked:
            stats["verifier_invoked"] += 1

        single_ix_to_verifier = len(instructions) == 1 and verifier_invoked

        # Check log success for verifier
        verifier_log_success = any(
            (f"Program {VERIFIER_PROGRAM_ID} success" in line) for line in logs
        )

        if single_ix_to_verifier and success and verifier_log_success:
            stats["single_ix_success"] += 1
            rd = meta.get("returnData")
            if not rd:
                stats["single_ix_success_no_return_data"] += 1
                if len(examples["no_return_data"]) < 10:
                    examples["no_return_data"].append(sig)
            else:
                rd_prog = rd.get("programId")
                if rd_prog == VERIFIER_PROGRAM_ID:
                    stats["single_ix_success_return_program_match"] += 1
                    if len(examples["match"]) < 10:
                        examples["match"].append(sig)
                else:
                    stats["single_ix_success_return_program_mismatch"] += 1
                    if len(examples["mismatch"]) < 10:
                        examples["mismatch"].append({"sig": sig, "rd_program": rd_prog})

        # be polite to the public RPC
        if (i + 1) % 10 == 0:
            time.sleep(0.4)

    print("Analysis of Chainlink verifier return-data behavior (recent mainnet txs)")
    print(json.dumps(stats, indent=2))
    print()
    if examples["match"]:
        print("Examples: single-instruction success with verifier as return-data program:")
        for s in examples["match"]:
            print(f"  {s}")
        print()
    if examples["no_return_data"]:
        print("Examples: single-instruction success with NO returnData:")
        for s in examples["no_return_data"]:
            print(f"  {s}")
        print()
    if examples["mismatch"]:
        print("Examples: single-instruction success with returnData program mismatch:")
        for e in examples["mismatch"]:
            print(f"  {e['sig']} -> returnData.programId={e['rd_program']}")


def main():
    limit = 60
    if len(sys.argv) > 1:
        try:
            limit = int(sys.argv[1])
        except Exception:
            pass
    sigs = get_signatures_for_address(VERIFIER_PROGRAM_ID, limit=limit)
    if not sigs:
        print("No signatures fetched. The program may be inactive or RPC is rate-limiting.")
        sys.exit(1)
    analyze_transactions(sigs)


if __name__ == "__main__":
    main()

