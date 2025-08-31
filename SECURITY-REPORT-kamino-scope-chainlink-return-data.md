## Title: Return-Data Origin Confusion in Scope Chainlink Refresh Enables Forged Price Updates

### Overview
A vulnerability in Kamino’s Scope oracle program allows a non-admin to inject forged Chainlink prices if the Chainlink Streams Verifier CPI succeeds without setting its own return data as the last writer. The handler `refresh_chainlink_price` trusts the last transaction return-data buffer without verifying the producer program ID and lacks an execution-context guard to restrict preceding instructions. Under the right runtime conditions, an attacker can cause Scope to decode attacker-controlled bytes as a valid Chainlink report and update on-chain prices.

### Affected component
- Program: `programs/scope`
- Entrypoint: `refresh_chainlink_price`
- Files:
  - `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`
  - `programs/scope/src/oracles/chainlink.rs`
  - `programs/scope/src/handlers/handler_refresh_prices.rs` (for safe pattern reference)
  - `programs/scope/src/utils/price_impl.rs` (ref-price guard)

### Root cause
- Unverified return-data origin: the handler reads the last return data via `get_return_data()` and ignores the producer `program_id`.
- Missing execution-context guard: unlike the batch refresh, the Chainlink handler does not require the instructions sysvar nor restrict prior instructions to ComputeBudget only.

### Code citations (root cause)

```86:211:programs/scope/src/handlers/handler_refresh_chainlink_price.rs
use solana_program::program::{get_return_data, invoke};
...
invoke(
    &chainlink_ix,
    &[
        ctx.accounts.verifier_account.to_account_info(),
        ctx.accounts.access_controller.to_account_info(),
        ctx.accounts.user.to_account_info(),
        ctx.accounts.config_account.to_account_info(),
    ],
)?;

let Some((_program_id, return_data)) = get_return_data() else {
    msg!("No report data found");
    return Err(error!(ScopeError::NoChainlinkReportData));
};

// return_data is decoded without asserting producer == Chainlink Verifier
match price_type {
    OracleType::Chainlink => {
        let chainlink_report = ReportDataV3::decode(&return_data)
            .map_err(|_| error!(ScopeError::InvalidChainlinkReportData))?;
        chainlink::update_price_v3(
            dated_price_ref,
            oracle_mapping,
            mapping_generic_data,
            &clock,
            &chainlink_report,
        )?;
    }
    // ... v7/v8/v9/v10 similar decode paths ...
}
```

```163:193:programs/scope/src/handlers/handler_refresh_prices.rs
/// Ensure that the refresh instruction is executed directly to avoid any manipulation:
fn check_execution_ctx(instruction_sysvar_account_info: &AccountInfo) -> Result<()> {
    let current_index: usize = load_current_index_checked(instruction_sysvar_account_info)?.into();
    // the current ix must be executed by our program id. otherwise, it's a CPI.
    if crate::ID != current_ix.program_id {
        return err!(ScopeError::RefreshInCPI);
    }
    // The current stack height must be the initial one. Otherwise, it's a CPI.
    if get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT {
        return err!(ScopeError::RefreshInCPI);
    }
    // Only allow ComputeBudget ixs before
    for ixn in 0..current_index {
        let ix = load_instruction_at_checked(ixn, instruction_sysvar_account_info)?;
        if ix.program_id != COMPUTE_BUDGET_ID {
            return err!(ScopeError::RefreshWithUnexpectedIxs);
        }
    }
}
```

```420:501:programs/scope/src/oracles/chainlink.rs
pub mod chainlink_streams_itf {
    ...
    pub const VERIFIER_PROGRAM_ID: Pubkey = pubkey!("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c");
    ...
    pub fn get_config_pda(report: &[u8]) -> Pubkey {
        Pubkey::find_program_address(&[&report[..32]], &VERIFIER_PROGRAM_ID).0
    }
}
```

```1:120:programs/scope/src/utils/price_impl.rs
pub const MAX_REF_RATIO_TOLERANCE_PCT: u64 = 5;
...
pub fn check_ref_price_difference(curr_price: Price, ref_price: Price) -> Result<()> {
    ...
    if absolute_diff * 100 > ref_price_decimal * MAX_REF_RATIO_TOLERANCE_PCT {
        return Err(ScopeError::PriceNotValid.into());
    }
    Ok(())
}
```

### Why the vulnerability exists
- Solana return data is global per-transaction and last-writer-wins. `get_return_data()` returns the most recent buffer written by any program in the transaction.
- The Chainlink refresh handler does not check that the return data’s `_program_id` equals the Chainlink Verifier program ID, so any preceding instruction (or later callee) that writes return data can influence what Scope decodes.
- The handler also lacks an instruction-context guard, allowing arbitrary preceding instructions (including attacker-controlled `set_return_data`) before the handler runs.

### Preconditions for exploitation
Exploitation requires that, on a successful `verify` CPI:
- the Chainlink Verifier does not set return data, or
- a callee on its success path overwrites return data after the verifier, or
- verifier behavior changes in future and it no longer writes return data last.

In those cases, a non-admin attacker can make Scope parse stale attacker-set bytes.

### Concrete exploit sketch (any signer)
1) Tx Instruction 0 (attacker program): call `set_return_data` with bytes encoding a valid `ReportDataV{3,7,8,9,10}` where:
   - `feed_id` equals the configured mapping pubkey,
   - timestamps are fresh and monotonic vs on-chain stored value,
   - market-status/“ripcord”/confidence constraints are satisfied,
   - price fields are attacker-chosen.
2) Tx Instruction 1: call `refresh_chainlink_price` with valid verifier accounts and a real `serialized_chainlink_report` so the verifier CPI succeeds.
   - If the verifier does not leave its own return data as last, Scope decodes the stale attacker buffer and updates the price.
3) Tx Instruction 2+: immediately call downstream protocols (e.g., Kamino lending) to profit from the manipulated price.

### Why existing validations don’t stop this
- Feed ID check binds only to the mapping pubkey, which the attacker can embed in forged bytes:

```82:93:programs/scope/src/oracles/chainlink.rs
fn validate_report_feed_id(feed_id: &FeedID, mapping: &Pubkey) -> ScopeResult<()> {
    if feed_id.0 != mapping.to_bytes() { return Err(ScopeError::PriceNotValid); }
    Ok(())
}
```

- Per-version validators are satisfiable by an attacker who controls the bytes:
  - v3: bid/ask spread vs `confidence_factor` (from mapping generic data)
  - v7/v9: monotonic timestamps
  - v8/v10: market-status and freshness

- Optional ref-price guard limits deviation to 5% relative to a configured ref price, and only if configured.

### Exposure in mainnet config (examples)
Representative Chainlink-backed entries (indicating use by Kamino products via `group_ids`):

```1377:1396:configs/mainnet/3NJYftD5sjVfxSnUdZ1wVML8f3aC6mp1CXCL6L7TnU8C.json
"228": {
  "label": "Chainlink SOL/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x0003b778...c0d2bc24f"
},
"230": {
  "label": "Chainlink USDC/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x00038f83...3e7bd992"
}
```

```1440:1473:configs/mainnet/3NJYftD5sjVfxSnUdZ1wVML8f3aC6mp1CXCL6L7TnU8C.json
"236": { "label": "Chainlink USDT/USD", "oracle_type": "Chainlink", "group_ids": [ 1, 2 ] },
"238": { "label": "Chainlink BTC/USD", "oracle_type": "Chainlink", "group_ids": [ 1, 2 ] },
"240": { "label": "Chainlink ETH/USD", "oracle_type": "Chainlink", "group_ids": [ 1, 2 ] }
```

RWA/xStocks also mapped via Chainlink variants, some with TWAP and cross-references.

### Exploitability assessment
- The missing origin check is an objective bug. Exploitability depends on the Chainlink Verifier’s runtime guarantee: if it always sets return data and remains last-writer on success, the stale buffer will be overwritten in practice. If any success path leaves non-verifier return data as last, exploitation is viable.
- Treat this as exploitable unless there is a strong and version-pinned verifier guarantee.

### Impact on Kamino products
- Lending (klend): inflate collateral or suppress debt prices → over-borrowing or avoiding liquidation; conversely, depress prices to trigger liquidations.
- YVaults/Farms: distorted NAV/share pricing; mis-rebalancing; fee/accounting errors.
- Composites/TWAP: manipulated samples can be selected (MostRecentOf) or absorbed into TWAP, extending effect.

### Recommended remediations (low risk)
1) Verify return-data origin immediately after `get_return_data()`:
   - Require `pid == VERIFIER_PROGRAM_ID`; else error.
2) Add instruction-context hardening for `refresh_chainlink_price` (mirror `check_execution_ctx`):
   - Require not-in-CPI and only ComputeBudget instructions before the current one via the Instructions sysvar.
3) Defense-in-depth (optional):
   - After decoding, validate that the expected config PDA derived from the report bytes matches the passed `config_account`.
   - Ensure ref-price guards are enabled for sensitive feeds and sourced from robust references.

### Proof-of-concept outline (devnet/test)
- Write a tiny program that sets crafted return data, then calls `refresh_chainlink_price` with a valid report that passes verifier.
- Observe whether the price is set from the attacker’s buffer when verifier is not last-writer on success.

### Appendix: Additional relevant code
- Feed ID and validators: see `programs/scope/src/oracles/chainlink.rs` lines 82–93, 169–340.
- MostRecentOf divergence checks: `programs/scope/src/oracles/most_recent_of.rs`.
- Ref-price tolerance (5%): `programs/scope/src/utils/price_impl.rs` lines 9–11, 34–53.

### Conclusion
The Chainlink single-feed refresh path trusts unverified transaction return data and lacks execution-context restrictions, enabling a return-data confusion attack under plausible verifier runtime conditions. Implementing a strict origin check and the existing execution-context guard pattern will eliminate the class and align the handler with the hardened batch refresh and Pyth Lazer flows.
