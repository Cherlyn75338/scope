# Scope return-data confusion PoC

This PoC demonstrates crafting a transaction that first writes attacker-chosen return data, then calls `refresh_chainlink_price` on a Scope staging deployment. If the Chainlink Verifier CPI succeeds but is not the last writer of return data, Scope will decode the forged buffer as a valid Chainlink report and update the price at the specified index.

## Components

- `attacker-program`: a minimal Solana program that sets return data to arbitrary bytes
- `report-encoder`: Rust CLI that encodes a Chainlink `ReportDataV*` buffer for a chosen price
- `client`: Node script that sends a two-instruction transaction: attacker ix, then `refresh_chainlink_price`

## Prerequisites

- Attacker program deployed on mainnet-beta; export `ATTACKER_PROGRAM_ID`
- Scope staging addresses on mainnet-beta:
  - `SCOPE_PROGRAM_ID`
  - `ORACLE_PRICES`, `ORACLE_MAPPINGS`, `ORACLE_TWAPS`
  - Chainlink Verifier config PDAs: `VERIFIER_CONFIG` (constant) and `CHAINLINK_CONFIG_PDA` (derived for your legit serialized report)
- A payer keypair with SOL balance (export `PAYER_SECRET_BASE58` base58-encoded secret key)
- A legitimate serialized Chainlink report that makes the Verifier CPI succeed (export `LEGIT_SERIALIZED_REPORT_HEX`)

## Build

- Attacker program: standard cargo-build-bpf, then deploy; note resulting program id
- Report encoder:

```
cargo run --manifest-path poc/report-encoder/Cargo.toml -- \
  --version 3 \
  --feed 9zjuRuvHMGb5M2SQoUMwZho3jcwxuHvRKkDpeKKMVH4U \
  --price_wei 1200000000000000000 \
  --ts $(date +%s)
```

Outputs hex string; set `FORGED_REPORT_HEX` to this value.

## Run the PoC transaction

```
cd poc/client
npm i
export SOLANA_URL=https://api.mainnet-beta.solana.com
export ATTACKER_PROGRAM_ID=... # deployed id
export SCOPE_PROGRAM_ID=...    # staging scope id
export ORACLE_PRICES=...
export ORACLE_MAPPINGS=...
export ORACLE_TWAPS=...
export CHAINLINK_CONFIG_PDA=...  # PDA for the legit report
export PAYER_SECRET_BASE58=...
export FORGED_REPORT_HEX=...      # from encoder (feed = index 230 mapping pubkey)
export LEGIT_SERIALIZED_REPORT_HEX=... # valid signed report bytes for verifier
export TOKEN_INDEX=230
node index.mjs
```

If exploitable in your environment, the transaction signature will be printed, and `oracle_prices` entry 230 will update toward the forged price (subject to 5% ref guard if configured).

## Notes

- The PoC relies on the Verifier CPI succeeding without being the final return-data writer. If the Verifier always sets and remains last writer, this vector is neutralized.
- Ensure the forged report’s `feed_id` matches the mapping pubkey at token index 230, and timestamps strictly increase.

