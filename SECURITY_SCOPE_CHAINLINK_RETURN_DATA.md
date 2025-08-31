## Kamino Scope Oracle — Chainlink return-data origin/confusion vulnerability

- Author: Security review
- Affected repo: `programs/scope` (Scope on-chain oracle aggregator)
- Severity: High (economic loss possible in lending/vaults if preconditions occur)
- Class: Return-data origin confusion + missing execution-context guard
- Status: Vulnerability confirmed in code; exploitability conditional on Chainlink verifier runtime behavior (see Preconditions)

### Executive summary

- Root cause: `refresh_chainlink_price` trusts the last transaction return-data buffer without verifying the producer program ID and has no execution-context guard. Solana return data is global and last-writer-wins for the entire transaction.
- Impact: If the Chainlink Streams verifier CPI succeeds while not leaving its own return data as the last writer, a non-admin can inject crafted return data that decodes as a valid Chainlink report and set arbitrary prices for any Chainlink-mapped feed in Scope, within per-type validation constraints. Downstream Kamino components (lending, vaults, farms) that consume these prices can be economically impacted.
- Exploit preconditions (any of these being true makes the attack viable):
  - Verifier’s success path sets no return data; or
  - A callee in the verifier’s success path sets return data after the verifier; or
  - Verifier behavior changes and it stops setting return data or being last writer on success.
- Non-admin: Any signer may invoke the refresh instruction; no elevated privileges required.
- Mitigations: Enforce `program_id == VERIFIER_PROGRAM_ID` on the buffer returned by `get_return_data()`; add the instruction-sysvar execution-context guard used elsewhere (not-in-CPI and only ComputeBudget instructions before the call).

### Scope and background

Scope is Kamino’s on-chain price oracle aggregator. It validates and consolidates prices from multiple sources (Pyth, Chainlink, DEXes…) into a single feed array, with per-index semantics defined off-chain (configs). Chainlink integration supports multiple report versions (v3, v7, v8, v9, v10).

Limitations noted by the project: an index-to-asset mapping is not stored on-chain; semantics are in the configs; up to 512 entries.

### Root-cause analysis (with code citations)

1) Handler invokes Chainlink verifier CPI, then blindly parses last return data, ignoring the source program ID

```55:92:programs/scope/src/handlers/handler_refresh_chainlink_price.rs
pub fn refresh_chainlink_price<'info>(
    ctx: Context<'_, '_, '_, 'info, RefreshChainlinkPrice<'info>>,
    token: u16,
    serialized_chainlink_report: Vec<u8>,
) -> Result<()> {
    // 1 - verify the report
    let program_id = ctx.accounts.verifier_program_id.key();
    let verifier_account = ctx.accounts.verifier_account.key();
    let access_controller = ctx.accounts.access_controller.key();
    let user = ctx.accounts.user.key();
    let config_account = ctx.accounts.config_account.key();
    // Create verification instruction
    let chainlink_ix = chainlink_streams_itf::verify(
        &program_id,
        &verifier_account,
        &access_controller,
        &user,
        &config_account,
        serialized_chainlink_report,
    );
    // Invoke the Verifier program
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
```

- Observation: `_program_id` is discarded. The handler accepts the most recent return-data written by any program in the transaction, not necessarily by the Chainlink verifier.

2) The handler decodes the buffer as a Chainlink report and updates the price; no origin assertion beyond feed ID and basic validations

```125:181:programs/scope/src/handlers/handler_refresh_chainlink_price.rs
// 2 - load the report and update the price
...
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
    OracleType::ChainlinkRWA => {
        let chainlink_report = ReportDataV8::decode(&return_data)
            .map_err(|_| error!(ScopeError::InvalidChainlinkReportData))?;
        chainlink::update_price_v8(...)?;
    }
    OracleType::ChainlinkNAV => { /* v9 */ }
    OracleType::ChainlinkX => { /* v10 */ }
    OracleType::ChainlinkExchangeRate => { /* v7 */ }
    _ => return Err(error!(ScopeError::BadTokenType)),
}
```

3) Feed-ID binding is necessary but insufficient if attacker controls bytes

```84:93:programs/scope/src/oracles/chainlink.rs
fn validate_report_feed_id(feed_id: &FeedID, mapping: &Pubkey) -> ScopeResult<()> {
    if feed_id.0 != mapping.to_bytes() {
        warn!("The chainlink report provided {} does not match the expected feed id in the mapping {}",
            feed_id.to_hex_string(),
            FeedID(mapping.to_bytes()).to_hex_string()
        );
        return Err(ScopeError::PriceNotValid);
    }
    Ok(())
}
```

- An attacker forging the return-data buffer can emit the correct `feed_id` for the targeted mapping and satisfy this check.

4) Per-version validations are attacker-satisfiable when bytes are attacker-controlled

- v3: Confidence bound is derived from mapping generic bytes; attacker can choose bid/ask consistent with a chosen price and confidence factor.

```169:210:programs/scope/src/oracles/chainlink.rs
pub fn update_price_v3(..., mapping_generic_data: &[u8], ..., chainlink_report: &ReportDataV3) -> ScopeResult<()> {
    validate_report_feed_id(&chainlink_report.feed_id, &mapping)?;
    let (unix_timestamp, last_updated_slot, generic_data) = validate_observations_timestamp(
        chainlink_report.observations_timestamp.into(),
        dated_price,
        clock,
    )?;
    let price_dec = chainlink_bigint_value_parse(&chainlink_report.benchmark_price)?;
    let bid_dec = chainlink_bigint_value_parse(&chainlink_report.bid)?;
    let ask_dec = chainlink_bigint_value_parse(&chainlink_report.ask)?;
    let spread = ask_dec - bid_dec;
    let confidence_factor: u32 = AnchorDeserialize::try_from_slice(&mapping_generic_data[..4]).unwrap();
    check_confidence_interval_decimal(price_dec, spread, confidence_factor)?;
    *dated_price = DatedPrice { price, last_updated_slot, unix_timestamp, generic_data };
    Ok(())
}
```

- v7, v8, v9, v10 follow similar patterns: timestamp monotonicity, market-status gating, and flags are present but can be chosen in a forged buffer to pass.

```212:236:programs/scope/src/oracles/chainlink.rs
pub fn update_price_v7(..., chainlink_report: &ReportDataV7) -> ScopeResult<()> {
    validate_report_feed_id(&chainlink_report.feed_id, &mapping)?;
    let (unix_timestamp, last_updated_slot, generic_data) = validate_observations_timestamp(
        chainlink_report.observations_timestamp.into(),
        dated_price,
        clock,
    )?;
    let price_dec = chainlink_bigint_value_parse(&chainlink_report.exchange_rate)?;
    *dated_price = DatedPrice { price, last_updated_slot, unix_timestamp, generic_data };
    Ok(())
}
```

```238:258:programs/scope/src/oracles/chainlink.rs
pub fn update_price_v8(..., mapping_generic_data: &[u8], ..., chainlink_report: &ReportDataV8) -> ScopeResult<()> {
    validate_report_feed_id(&chainlink_report.feed_id, &mapping)?;
    let (unix_timestamp, last_updated_slot, generic_data) = validate_observations_timestamp(...)?;
    validate_report_based_on_market_status(
        chainlink_report.market_status,
        chainlink_report.last_update_timestamp,
        mapping_generic_data,
        clock,
    )?;
    let price_dec = chainlink_bigint_value_parse(&chainlink_report.mid_price)?;
    *dated_price = DatedPrice { price, last_updated_slot, unix_timestamp, generic_data };
    Ok(())
}
```

```272:303:programs/scope/src/oracles/chainlink.rs
pub fn update_price_v9(..., chainlink_report: &ReportDataV9) -> ScopeResult<()> {
    validate_report_feed_id(&chainlink_report.feed_id, &mapping)?;
    let (unix_timestamp, last_updated_slot, generic_data) = validate_observations_timestamp(...)?;
    let ripcord = ReportDataV9RipcordFlag::try_from(chainlink_report.ripcord)?;
    if ripcord == ReportDataV9RipcordFlag::Paused { return Err(ScopeError::PriceNotValid); }
    let price_dec = chainlink_bigint_value_parse(&chainlink_report.nav_per_share)?;
    *dated_price = DatedPrice { price, last_updated_slot, unix_timestamp, generic_data };
    Ok(())
}
```

```305:340:programs/scope/src/oracles/chainlink.rs
pub fn update_price_v10(..., mapping_generic_data: &[u8], ..., chainlink_report: &ReportDataV10) -> ScopeResult<()> {
    validate_report_feed_id(&chainlink_report.feed_id, &mapping)?;
    let (unix_timestamp, last_updated_slot, generic_data) = validate_observations_timestamp(...)?;
    validate_report_based_on_market_status(
        chainlink_report.market_status,
        chainlink_report.last_update_timestamp,
        mapping_generic_data,
        clock,
    )?;
    let price_dec = chainlink_bigint_value_parse(&chainlink_report.price)?;
    let current_multiplier_dec = chainlink_bigint_value_parse(&chainlink_report.current_multiplier)?;
    let multiplied_price: Price = (price_dec * current_multiplier_dec).into();
    *dated_price = DatedPrice { price: multiplied_price, last_updated_slot, unix_timestamp, generic_data };
    Ok(())
}
```

5) The constants for expected verifier IDs exist but are not used to validate the return-data origin

```430:442:programs/scope/src/oracles/chainlink.rs
#[cfg(not(feature = "devnet"))]
pub const ACCESS_CONTROLLER_PUBKEY: Pubkey =
    pubkey!("7mSn5MoBjyRLKoJShgkep8J17ueGG8rYioVAiSg5YWMF");
...
pub const VERIFIER_CONFIG_PUBKEY: Pubkey =
    pubkey!("HJR45sRiFdGncL69HVzRK4HLS2SXcVW3KeTPkp2aFmWC");

pub const VERIFIER_PROGRAM_ID: Pubkey = pubkey!("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c");
```

6) Contrast: hardened execution-context guard exists for the batch refresh path, but not used in Chainlink handler

```163:191:programs/scope/src/handlers/handler_refresh_prices.rs
/// Ensure that the refresh instruction is executed directly to avoid any manipulation:
///
/// - Check that the current instruction is executed by our program id (not in CPI).
/// - Check that instructions preceding the refresh are compute budget instructions.
fn check_execution_ctx(instruction_sysvar_account_info: &AccountInfo) -> Result<()> {
    let current_index: usize = load_current_index_checked(instruction_sysvar_account_info)?.into();
    // 1- Check that the current instruction is executed by our program id (not in CPI).
    let current_ix = load_instruction_at_checked(current_index, instruction_sysvar_account_info)?;
    // the current ix must be executed by our program id. otherwise, it's a CPI.
    if crate::ID != current_ix.program_id { return err!(ScopeError::RefreshInCPI); }
    // The current stack height must be the initial one. Otherwise, it's a CPI.
    if get_stack_height() > TRANSACTION_LEVEL_STACK_HEIGHT { return err!(ScopeError::RefreshInCPI); }
    // 2- Check that instructions preceding the refresh are compute budget instructions.
    for ixn in 0..current_index {
        let ix = load_instruction_at_checked(ixn, instruction_sysvar_account_info)?;
        if ix.program_id != COMPUTE_BUDGET_ID { return err!(ScopeError::RefreshWithUnexpectedIxs); }
    }
    Ok(())
}
```

7) Optional ref-price divergence check exists (5% bound vs configured ref)

```201:209:programs/scope/src/handlers/handler_refresh_chainlink_price.rs
// check that the price is close enough to the ref price if there is a ref price
if oracle_mappings.ref_price[token_idx] != u16::MAX {
    let new_price = oracle_prices.prices[token_idx].price;
    let ref_price =
        oracle_prices.prices[usize::from(oracle_mappings.ref_price[token_idx])].price;
    check_ref_price_difference(new_price, ref_price)?;
}
```

### Why this is a vulnerability on Solana

- Solana’s `get_return_data()` returns the last buffer set by any program during the entire transaction. It is global, not scoped per CPI, and is “last writer wins.”
- The handler ignores the returned producer `_program_id`. Without verifying that it equals the Chainlink verifier’s program ID, any last writer in the transaction can provide the buffer that Scope will decode.
- The Chainlink handler also lacks the instruction-sysvar guard, so preceding top-level instructions can be arbitrary.

These two conditions together enable a return-data confusion attack if the verifier does not write the expected buffer as the final writer on a successful path.

### Exploitability assessment

- Intended vs bug: This is not intended. The batch refresh path has a defensive execution-context guard; the Chainlink handler lacks both origin verification and context guard.
- Exploitability on mainnet: Conditional. If the Chainlink Streams verifier’s `verify` succeeds and:
  - writes no return data, or
  - a callee overwrites return data after it, or
  - its behavior changes later,
  then Scope will parse stale attacker-set bytes and write the forged price.
- Note: If the verifier always sets and remains last writer on success, it neutralizes this specific stale-data vector today; however, the missing origin check remains a bug and future-behavior changes or callee writes could re-enable it.

### Concrete exploitation path (any signer)

1) Craft a transaction:
   - Instruction 0: Call an attacker-controlled program that executes `set_return_data` with bytes that decode as a valid `ReportDataV{3,7,8,9,10}` for the targeted feed. Set:
     - `feed_id` equal to the Scope mapping pubkey for that index;
     - timestamps strictly increasing vs on-chain stored last observation;
     - market status and flags acceptable (e.g., Open, not Paused);
     - values consistent with confidence/spread constraints;
     - attacker-chosen price values.
   - Instruction 1: Call `refresh_chainlink_price` with a real serialized report that makes the Chainlink verifier CPI succeed. If the verifier does not set (or is not last to set) its own return data, Scope reads the attacker’s stale buffer and writes the forged price to `OraclePrices`.
   - Instruction 2+: Immediately consume the corrupted price in downstream Kamino instructions (e.g., over-borrow in lending) within the same transaction.

2) No admin needed; caller is any signer.

3) Optional ref-price guard, if configured, caps per-update deviation to 5% vs the ref source. This reduces but does not eliminate impact; it can still be walked if repeatedly invoked or bypassed if the ref source is also influenced.

### Downstream impact (Kamino consumers)

- Lending (klend): Inflate collateral prices to increase borrow capacity; suppress debt prices to avoid liquidations; trigger unwanted liquidations if manipulated downward.
- Vaults/strategies (yvaults): NAV/share mispricing, fee/accounting distortions, mis-rebalances.
- Farms/other: Incentive misallocations, incorrect reward accounting.
- Aggregations: If Chainlink is a constituent of `MostRecentOf`, a forged-but-fresh sample may be selected if within divergence bounds.
- TWAP: If enabled, forged samples enter the EMA and prolong effects until decayed.

Illustrative mainnet exposure (Chainlink-mapped entries):

```1377:1396:configs/mainnet/3NJYftD5sjVfxSnUdZ1wVML8f3aC6mp1CXCL6L7TnU8C.json
"228": {
  "label": "Chainlink SOL/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x0003b778d3f6b2ac4991302b89cb313f99a42467d6c9c5f96f57c29c0d2bc24f",
  "confidence_factor": 50
},
...
"230": {
  "label": "Chainlink USDC/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x00038f83323b6b08116d1614cf33a9bd71ab5e0abf0c9f1b783a74a43e7bd992",
  "confidence_factor": 200
}
```

```1440:1474:configs/mainnet/3NJYftD5sjVfxSnUdZ1wVML8f3aC6mp1CXCL6L7TnU8C.json
"236": {
  "label": "Chainlink USDT/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x0003a910a43485e0685ff5d6d366541f5c21150f0634c5b14254392d1a1c06db",
  "confidence_factor": 200
},
...
"240": {
  "label": "Chainlink ETH/USD",
  "oracle_type": "Chainlink",
  "group_ids": [ 1, 2 ],
  "chainlink_feed_id": "0x000362205e10b3a147d02792eccee483dca6c7b44ecce7012cb8c6e0b68b3ae9",
  "confidence_factor": 50
}
```

### Definitive mitigations (low risk)

1) Verify return-data origin immediately after `get_return_data()`

- Enforce that the producer program ID equals `VERIFIER_PROGRAM_ID` and reject otherwise. This binds the parsed bytes to the Chainlink verifier’s output.

```rust
// Immediately after invoking the verifier
let Some((pid, return_data)) = get_return_data() else {
    msg!("No report data found");
    return Err(error!(ScopeError::NoChainlinkReportData));
};
require_keys_eq!(pid, VERIFIER_PROGRAM_ID, ScopeError::NoChainlinkReportData);
```

2) Add execution-context guard to `refresh_chainlink_price`

- Require the instructions sysvar account and reuse the same `check_execution_ctx` semantics as in `refresh_price_list` (not in CPI; only ComputeBudget instructions allowed before current index).

3) Defense in depth (optional)

- Recompute/assert the config PDA from the decoded report bytes equals the passed `config_account` (mirrors the verifier’s checks).
- Ensure ref-price guards are configured for Chainlink-mapped tokens to robust references (e.g., Pyth) where possible.
- Add monitoring/alerts for large deviations or frequent ref-bound hits.

### Is it exploitable “100%” on mainnet?

- Not provable as 100% without authoritative guarantees of verifier runtime behavior. The attack is viable if (and only if) the verifier CPI succeeds without leaving its own return data as the last writer.
- Regardless, the missing origin check is a real vulnerability and should be fixed. Future updates to the verifier or nested CPI behavior could change last-writer semantics at any time.

### Affected components

- `programs/scope/src/handlers/handler_refresh_chainlink_price.rs`
- `programs/scope/src/oracles/chainlink.rs` (update helpers and constants)
- Consumers: Kamino lending (klend), yvaults, possibly farms/others using Scope Chainlink-mapped entries per configs.

### Recommended remediation plan

- Implement origin check and execution-context guard in a single release.
- Add tests:
  - Positive: normal verifier path with valid report still succeeds.
  - Negative: preceding instruction sets return data from a non-verifier PID → instruction must fail.
  - Negative: CPI context invocation → instruction must fail (guard).
- Perform a short on-chain audit of recent verifier transactions to confirm current last-writer behavior; keep as regression-monitoring item.

### Appendix: Verifier interface in repo

```465:499:programs/scope/src/oracles/chainlink.rs
pub fn verify(
    program_id: &Pubkey,
    verifier_account: &Pubkey,
    access_controller_account: &Pubkey,
    user: &Pubkey,
    report_config_account: &Pubkey,
    signed_report: Vec<u8>,
) -> Instruction { /* constructs the CPI to verifier */ }

pub fn get_config_pda(report: &[u8]) -> Pubkey {
    Pubkey::find_program_address(&[&report[..32]], &VERIFIER_PROGRAM_ID).0
}
```

### Appendix: Optional ref-price guard in handler

```201:209:programs/scope/src/handlers/handler_refresh_chainlink_price.rs
if oracle_mappings.ref_price[token_idx] != u16::MAX {
    let new_price = oracle_prices.prices[token_idx].price;
    let ref_price = oracle_prices.prices[usize::from(oracle_mappings.ref_price[token_idx])].price;
    check_ref_price_difference(new_price, ref_price)?;
}
```

### Final verdict

- The Chainlink refresh handler contains a real vulnerability: it trusts unverified transaction return data and lacks execution-context hardening. Exploitability is conditional on verifier last-writer behavior today, but relying on external guarantees is brittle. Implementing the origin check and the instruction-sysvar guard closes the class with minimal risk and future-proofs the integration.
