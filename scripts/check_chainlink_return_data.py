#!/usr/bin/env python3
import json
import os
import sys
import time
from typing import Any, Dict, List, Optional, Tuple
from urllib.request import Request, urlopen


RPC_URL = os.environ.get("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com")
VERIFIER_PROGRAM_ID = "Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c"
SCOPE_PROGRAM_ID = os.environ.get("SCOPE_PROGRAM_ID")  # Set to Kamino Scope program id to target Scope calls


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
        # CPI path via log detection
        "cpi_success": 0,
        "cpi_success_return_program_match": 0,
        "cpi_success_no_return_data": 0,
        "cpi_success_return_program_mismatch": 0,
        # Scope-targeted path (top-level ix program == SCOPE_PROGRAM_ID and inner ix to verifier)
        "scope_calls": 0,
        "scope_verifier_cpi_success": 0,
        "scope_verifier_cpi_return_program_match": 0,
        "scope_verifier_cpi_no_return_data": 0,
        "scope_verifier_cpi_return_program_mismatch": 0,
        # Auto-detected Scope-like (top-level ix that inner-calls verifier and logs include refresh+chainlink)
        "auto_scope_calls": 0,
        "auto_scope_return_program_match": 0,
        "auto_scope_no_return_data": 0,
        "auto_scope_return_program_mismatch": 0,
    }
    examples = {
        "no_return_data": [],
        "mismatch": [],
        "match": [],
        "cpi_no_return_data": [],
        "cpi_mismatch": [],
        "cpi_match": [],
        "scope_match": [],
        "scope_no_return_data": [],
        "scope_mismatch": [],
        "auto_scope_match": [],
        "auto_scope_no_return_data": [],
        "auto_scope_mismatch": [],
    }
    auto_scope_program_ids: Dict[str, int] = {}

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
        verifier_log_success = any((f"Program {VERIFIER_PROGRAM_ID} success" in line) for line in logs)

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

        # CPI path: verifier success appears in logs anywhere in the tx
        if not single_ix_to_verifier and success and verifier_log_success:
            stats["cpi_success"] += 1
            rd = meta.get("returnData")
            if not rd:
                stats["cpi_success_no_return_data"] += 1
                if len(examples["cpi_no_return_data"]) < 10:
                    examples["cpi_no_return_data"].append(sig)
            else:
                rd_prog = rd.get("programId")
                if rd_prog == VERIFIER_PROGRAM_ID:
                    stats["cpi_success_return_program_match"] += 1
                    if len(examples["cpi_match"]) < 10:
                        examples["cpi_match"].append(sig)
                else:
                    stats["cpi_success_return_program_mismatch"] += 1
                    if len(examples["cpi_mismatch"]) < 10:
                        examples["cpi_mismatch"].append({"sig": sig, "rd_program": rd_prog})

        # Scope-targeted: top-level ix to SCOPE_PROGRAM_ID, with inner ix to verifier
        # Note: meta.innerInstructions indexes correspond to top-level instruction indices
        # Helper: does log mention scope refresh chainlink instruction semantics
        def logs_indicate_scope_refresh(logs_list: List[str]) -> bool:
            for line in logs_list:
                l = line.lower()
                if "instruction:" in l and "refresh" in l and "chainlink" in l:
                    return True
            return False

        if SCOPE_PROGRAM_ID:
            # find all top-level indices where program == SCOPE_PROGRAM_ID
            scope_tl_indices = [idx for idx, ix in enumerate(instructions) if program_id_for_ix(ix, keys) == SCOPE_PROGRAM_ID]
            for tl_idx in scope_tl_indices:
                stats["scope_calls"] += 1
                inner_list = (meta.get("innerInstructions") or [])
                # find inner instructions under this top-level index
                inner_for_idx: List[Dict[str, Any]] = []
                for inner in inner_list:
                    if inner.get("index") == tl_idx:
                        inner_for_idx = inner.get("instructions") or []
                        break
                # resolve program ids for inner instructions
                inner_has_verifier = False
                for iix in inner_for_idx:
                    # inner instruction encoding uses programIdIndex
                    pid = None
                    if "programIdIndex" in iix:
                        try:
                            idx = int(iix["programIdIndex"])
                            if 0 <= idx < len(keys):
                                pid = keys[idx]
                        except Exception:
                            pid = None
                    elif "programId" in iix:
                        pid = iix["programId"]
                    if pid == VERIFIER_PROGRAM_ID:
                        inner_has_verifier = True
                        break
                if inner_has_verifier and success and verifier_log_success:
                    stats["scope_verifier_cpi_success"] += 1
                    rd = meta.get("returnData")
                    if not rd:
                        stats["scope_verifier_cpi_no_return_data"] += 1
                        if len(examples["scope_no_return_data"]) < 10:
                            examples["scope_no_return_data"].append(sig)
                    else:
                        rd_prog = rd.get("programId")
                        if rd_prog == VERIFIER_PROGRAM_ID:
                            stats["scope_verifier_cpi_return_program_match"] += 1
                            if len(examples["scope_match"]) < 10:
                                examples["scope_match"].append(sig)
                        else:
                            stats["scope_verifier_cpi_return_program_mismatch"] += 1
                            if len(examples["scope_mismatch"]) < 10:
                                examples["scope_mismatch"].append({"sig": sig, "rd_program": rd_prog})

        # Auto-detect Scope-like: any top-level ix with inner verifier CPI and logs show refresh+chainlink
        # Also collect candidate program ids
        inner_list = (meta.get("innerInstructions") or [])
        if inner_list:
            # map tl index to whether it calls verifier
            tl_to_has_verifier: Dict[int, bool] = {}
            for inner in inner_list:
                tl_index = inner.get("index")
                for iix in (inner.get("instructions") or []):
                    pid = None
                    if "programIdIndex" in iix:
                        try:
                            idx = int(iix["programIdIndex"])
                            if 0 <= idx < len(keys):
                                pid = keys[idx]
                        except Exception:
                            pid = None
                    elif "programId" in iix:
                        pid = iix["programId"]
                    if pid == VERIFIER_PROGRAM_ID:
                        tl_to_has_verifier[tl_index] = True
                        break
            if tl_to_has_verifier and logs_indicate_scope_refresh(logs):
                for tl_idx, has_ver in tl_to_has_verifier.items():
                    if not has_ver:
                        continue
                    pid = program_id_for_ix(instructions[tl_idx], keys)
                    if pid:
                        auto_scope_program_ids[pid] = auto_scope_program_ids.get(pid, 0) + 1
                    stats["auto_scope_calls"] += 1
                    rd = meta.get("returnData")
                    if not rd:
                        stats["auto_scope_no_return_data"] += 1
                        if len(examples["auto_scope_no_return_data"]) < 10:
                            examples["auto_scope_no_return_data"].append(sig)
                    else:
                        rd_prog = rd.get("programId")
                        if rd_prog == VERIFIER_PROGRAM_ID:
                            stats["auto_scope_return_program_match"] += 1
                            if len(examples["auto_scope_match"]) < 10:
                                examples["auto_scope_match"].append(sig)
                        else:
                            stats["auto_scope_return_program_mismatch"] += 1
                            if len(examples["auto_scope_mismatch"]) < 10:
                                examples["auto_scope_mismatch"].append({"sig": sig, "rd_program": rd_prog})

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
        print()
    if examples["cpi_match"]:
        print("Examples: CPI success with verifier as final return-data program:")
        for s in examples["cpi_match"]:
            print(f"  {s}")
        print()
    if examples["cpi_no_return_data"]:
        print("Examples: CPI success with NO final returnData:")
        for s in examples["cpi_no_return_data"]:
            print(f"  {s}")
        print()
    if examples["cpi_mismatch"]:
        print("Examples: CPI success with final returnData program mismatch:")
        for e in examples["cpi_mismatch"]:
            print(f"  {e['sig']} -> returnData.programId={e['rd_program']}")
        print()
    if SCOPE_PROGRAM_ID:
        if examples["scope_match"]:
            print("Examples: Scope tx (top-level) with verifier CPI success and verifier as final return-data program:")
            for s in examples["scope_match"]:
                print(f"  {s}")
            print()
        if examples["scope_no_return_data"]:
            print("Examples: Scope tx (top-level) with verifier CPI success and NO final returnData:")
            for s in examples["scope_no_return_data"]:
                print(f"  {s}")
            print()
        if examples["scope_mismatch"]:
            print("Examples: Scope tx (top-level) with verifier CPI success and final returnData program mismatch:")
            for e in examples["scope_mismatch"]:
                print(f"  {e['sig']} -> returnData.programId={e['rd_program']}")
    if auto_scope_program_ids:
        print()
        print("Auto-detected candidate Scope program IDs (top-level ix that CPI-calls verifier and logs show refresh+chainlink):")
        for pid, count in auto_scope_program_ids.items():
            print(f"  {pid}: {count} txs")
        print()
    if examples["auto_scope_match"]:
        print("Examples: Auto-scope tx with verifier as final return-data program:")
        for s in examples["auto_scope_match"]:
            print(f"  {s}")
        print()
    if examples["auto_scope_no_return_data"]:
        print("Examples: Auto-scope tx with NO final returnData:")
        for s in examples["auto_scope_no_return_data"]:
            print(f"  {s}")
        print()
    if examples["auto_scope_mismatch"]:
        print("Examples: Auto-scope tx with final returnData program mismatch:")
        for e in examples["auto_scope_mismatch"]:
            print(f"  {e['sig']} -> returnData.programId={e['rd_program']}")


def main():
    limit = 60
    if len(sys.argv) > 1:
        try:
            limit = int(sys.argv[1])
        except Exception:
            pass
    target_address = SCOPE_PROGRAM_ID or VERIFIER_PROGRAM_ID
    sigs = get_signatures_for_address(target_address, limit=limit)
    if not sigs:
        print("No signatures fetched. The program may be inactive or RPC is rate-limiting.")
        sys.exit(1)
    analyze_transactions(sigs)


if __name__ == "__main__":
    main()

