#![allow(clippy::result_large_err)]
use anchor_lang::prelude::*;
use solana_program::{program::get_return_data, pubkey};

declare_id!(pubkey!("HarnESS111111111111111111111111111111111111"));

#[program]
pub mod scope_harness {
    use super::*;

    pub fn refresh_chainlink_like(_ctx: Context<Ctx>) -> Result<()> {
        let Some((_pid, data)) = get_return_data() else {
            return err!(ErrorCode::NoReturnData);
        };
        // Accept any non-empty data to simulate the vulnerable parsing step
        if data.is_empty() {
            return err!(ErrorCode::InvalidReport);
        }
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ctx {}

#[error_code]
pub enum ErrorCode {
    #[msg("No return data")] NoReturnData,
    #[msg("Invalid report")] InvalidReport,
}

