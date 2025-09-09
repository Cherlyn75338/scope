use anyhow::{anyhow, Result};
use chainlink_streams_report::report::{v3::ReportDataV3, v7::ReportDataV7, v8::ReportDataV8, v9::ReportDataV9, v10::ReportDataV10};
use clap::Parser;
use num_bigint::BigInt;

#[derive(Parser, Debug)]
#[command(author, version, about="Encode a Chainlink Streams ReportDataV* pretending to be verifier output")] 
struct Args {
    /// Version: 3,7,8,9,10
    #[arg(long)]
    version: u8,
    /// feed id as base58 pubkey (mapping pubkey)
    #[arg(long)]
    feed: String,
    /// price as integer with 18 decimals (e.g. 1 USDC = 1e18)
    #[arg(long)]
    price_wei: String,
    /// observations timestamp (unix seconds, strictly increasing)
    #[arg(long)]
    ts: u64,
    /// extra per-version knobs (ignored if not applicable)
    #[arg(long, default_value_t = 0)]
    market_status: u32,
    #[arg(long, default_value_t = 0)]
    last_update_ns: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let feed_pk = bs58::decode(&args.feed).into_vec().map_err(|_| anyhow!("bad feed b58"))?;
    if feed_pk.len() != 32 { return Err(anyhow!("feed must be 32 bytes")); }

    let price_big = BigInt::parse_bytes(args.price_wei.as_bytes(), 10).ok_or_else(|| anyhow!("bad price_wei"))?;

    let out = match args.version {
        3 => {
            let mut r = ReportDataV3::default();
            r.feed_id.0.copy_from_slice(&feed_pk);
            r.observations_timestamp = args.ts.into();
            r.benchmark_price = price_big.clone();
            r.bid = price_big.clone();
            r.ask = price_big.clone();
            r.encode()
        }
        7 => {
            let mut r = ReportDataV7::default();
            r.feed_id.0.copy_from_slice(&feed_pk);
            r.observations_timestamp = args.ts.into();
            r.exchange_rate = price_big.clone();
            r.encode()
        }
        8 => {
            let mut r = ReportDataV8::default();
            r.feed_id.0.copy_from_slice(&feed_pk);
            r.observations_timestamp = args.ts.into();
            r.market_status = args.market_status;
            r.last_update_timestamp = args.last_update_ns;
            r.mid_price = price_big.clone();
            r.encode()
        }
        9 => {
            let mut r = ReportDataV9::default();
            r.feed_id.0.copy_from_slice(&feed_pk);
            r.observations_timestamp = args.ts.into();
            r.ripcord = 0;
            r.nav_per_share = price_big.clone();
            r.encode()
        }
        10 => {
            let mut r = ReportDataV10::default();
            r.feed_id.0.copy_from_slice(&feed_pk);
            r.observations_timestamp = args.ts.into();
            r.market_status = args.market_status;
            r.last_update_timestamp = args.last_update_ns;
            r.price = price_big.clone();
            r.current_multiplier = BigInt::from(1u8);
            r.encode()
        }
        _ => return Err(anyhow!("unsupported version")),
    };

    println!("{}", hex::encode(out));
    Ok(())
}

