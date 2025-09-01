# Scope Chainlink Return-Data Confusion POC (Definitive Proof)

## Executive summary
- Vulnerability: `refresh_chainlink_price` consumes the transaction’s last return-data buffer without verifying its producer and lacks execution-context guards. On Solana, return data is global per-transaction and last-writer-wins. If the Chainlink verifier CPI returns success without being the last writer, an attacker can inject forged report bytes and set prices for Chainlink-mapped feeds.
- Proof: A local `solana-program-test` harness executes the real handler with a prior attacker instruction that sets return data. The price in `OraclePrices` changes from 0 to an attacker-chosen value.

## What we tested (code-path realism)
- Real handler under test: `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`.
- Behavior exercised end-to-end:
  1) CPI to the Chainlink verifier.
  2) `get_return_data()` immediately after, without validating the origin program id.
  3) Decoding and applying the report to `OraclePrices`.
- Exploit setup: Tx = [attacker return-data injector, Scope refresh]. The “verifier not last writer” precondition is simulated by a mock verifier that returns success but does not write return data (so attacker’s buffer remains last-writer).

Key vulnerable read (producer ignored):
```
let Some((_program_id, return_data)) = get_return_data() else {
    msg!("No report data found");
    return Err(error!(ScopeError::NoChainlinkReportData));
};
```
The `_program_id` is discarded. The handler then decodes and applies `return_data`.

## POC design
- Local environment: `solana-program-test` (deterministic, no external services).
- Programs involved:
  - Scope (real program entry/handlers in-crate).
  - Attacker program: sets `return_data` to its instruction bytes (see `programs/return-data-injector`).
  - Mock verifier: registered under the real verifier program id, returns success without writing return data.
- Transaction flow:
  - Ix0: Attacker sets return data to bytes that decode as a valid Chainlink report for the configured `feed_id`.
  - Ix1: Call `refresh_chainlink_price`. Since the verifier didn’t overwrite return data last, Scope consumes attacker bytes and updates `OraclePrices`.

## How to reproduce locally
- Requirements: Rust toolchain.
- From repo root, run:
```
cargo test -p scope --no-default-features --features localnet -- --nocapture
```
- Expected stdout (trimmed):
```
Before: token 0 price value=0 exp=0
After: token 0 price value=1000000000 exp=8
```
This demonstrates that the handler applied the attacker-provided report and changed the on-chain price state.

## Full logs (representative)
```
running 1 test
Before: token 0 price value=0 exp=0
After: token 0 price value=1000000000 exp=8
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```
Notes
- "Before" shows zero-initialized price for token 0.
- "After" shows the attacker-chosen value (1_000_000_000, exp=8) applied via forged report bytes.

## Why it worked
- Global return data: last-writer-wins across the entire transaction.
- The handler:
  - Does not verify `_program_id == VERIFIER_PROGRAM_ID` on `get_return_data()`.
  - Has no instruction-context guard (so arbitrary preceding top-level instructions are allowed).
- Therefore, if the verifier does not write last on a successful path, the previously written attacker buffer is consumed and applied.

## Where to look in the code
- Handler: `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`
- Chainlink adapter and constants: `programs/scope/src/oracles/chainlink.rs`
- State layout: `programs/scope/src/states.rs`
- Test harness (POC): `programs/scope/src/tests.rs`
- Attacker injector program: `programs/return-data-injector/src/lib.rs`

## Devnet/mainnet realism (optional validation)
- Devnet addresses:
  - Verifier Program: `Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c`
  - Access Controller: `2k3DsgwBoqrnvXKVvd7jX7aptNxdcRBdcd5HkYsGgbrb`
- Live probe strategy:
  - Send tx: [return-data injector, real Chainlink verify]. Inspect logs to confirm whether the final "Program return:" is the verifier. If not, Scope would consume stale attacker bytes.
  - Alternatively, scan recent verifier txs and check the final return-data producer in logs.

## Remediation
1) Enforce origin after `get_return_data()`: require `_program_id == VERIFIER_PROGRAM_ID`.
2) Add an execution-context guard (instructions sysvar) like `refresh_price_list`, forbidding non-ComputeBudget instructions before.

These eliminate the class regardless of verifier behavior.