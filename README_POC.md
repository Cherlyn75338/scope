PoC: return-data confusion in Scope refresh_chainlink_price

This PoC scaffolds:
- attacker-rd: a tiny Anchor program that sets arbitrary return data
- poc-ts: a TypeScript client to build transactions that exercise three scenarios

Devnet IDs
- Scope devnet program id: 3Vw8Ngkh1MVJTPHthmUbmU2XKtFEkjYvJzMqrv2rh9yX
- Chainlink Streams verifier program id (as used in repo): Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c
- Access controller (devnet): 2k3DsgwBoqrnvXKVvd7jX7aptNxdcRBdcd5HkYsGgbrb
- Verifier config PDA: HJR45sRiFdGncL69HVzRK4HLS2SXcVW3KeTPkp2aFmWC

Setup
1) Install Rust and Node.js
2) Build attacker program locally (deployment optional if you already have one):
   - The crate is at programs/attacker-rd
   - Deploy to devnet with Anchor or Solana CLI and note the program id
3) Install TS deps:
   - cd poc-ts && npm i && npm run build

Preparing a test Scope feed
1) Create accounts sized per utils/consts.rs
   - CONFIGURATION_SIZE: 10232
   - ORACLE_MAPPING_SIZE: 29696
   - ORACLE_PRICES_SIZE: 28704
   - ORACLE_TWAPS_SIZE: 344128
   - TOKEN_METADATA_SIZE: 86016
2) Call initialize(admin, feed_name)
3) Call update_mapping to set token index to a Chainlink type (26/34/35/37/38) with:
   - price_info = the feed id pubkey (Pubkey that must equal Report.feed_id)
   - generic[0..4] set appropriately (confidence factor for v3; market status behavior for v8/v10)

Running tests
In poc-ts/src/index.ts:
 - Set ATTACKER_PROGRAM_ID to your deployed attacker program
 - Fill buildScopeRefreshChainlinkPriceIx with proper accounts (see IDL) and serialized report bytes
 - Provide a real signed Chainlink Streams report for the feed you mapped, so the verifier CPI succeeds

Test patterns
1) Pre-CPI stale writer
   - Ix0: attacker-rd::set_bytes(malicious_report_bytes)
   - Ix1: scope::refresh_chainlink_price(token, real_serialized_report)
   Expectation: If the verifier succeeds but does not set last return data, Scope reads malicious bytes

2) Post-CPI overwrite
   - Ix0: scope::refresh_chainlink_price(token, real_serialized_report)
   - Ix1: attacker-rd::set_bytes(malicious_report_bytes)
   Expectation: If Scope reads after Ix0’s internal CPI and any callee overwrites return data, malicious wins

3) Execution-context guard absence
   - Ix0: attacker-rd::set_bytes(...)
   - Ix1: scope::refresh_chainlink_price(...)
   Expectation: Succeeds (no guard). Contrast with refresh_price_list which enforces an instruction-sysvar guard

Notes on malicious bytes
- Must bincode-decode to the appropriate ReportDataV{3,7,8,9,10}
- Set feed_id == the mapping pubkey at your index
- Set timestamps strictly increasing vs on-chain
- Set flags/market status acceptable
- Price within optional ref-price 5% guard if configured

Chainlink docs
- Using data feeds on Solana: https://docs.chain.link/data-feeds/solana/using-data-feeds-solana

Mitigations to verify
- Add producer program-id check against VERIFIER_PROGRAM_ID on get_return_data()
- Add execution-context guard (same as refresh_price_list)

