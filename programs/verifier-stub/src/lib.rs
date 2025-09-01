#![allow(clippy::result_large_err)]
use anchor_lang::prelude::*;
use solana_program::{program::set_return_data, pubkey};

declare_id!(pubkey!("Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c"));

#[program]
pub mod verifier_stub {
    use super::*;

    pub fn verify_with_return_data(_ctx: Context<VerifyCtx>, data: Vec<u8>) -> Result<()> {
        set_return_data(&data);
        Ok(())
    }

    pub fn verify_without_return_data(_ctx: Context<VerifyCtx>, _data: Vec<u8>) -> Result<()> {
        Ok(())
    }

    pub fn verify_then_overwrite_return_data(_ctx: Context<VerifyCtx>, verifier_data: Vec<u8>, overwrite: Vec<u8>) -> Result<()> {
        set_return_data(&verifier_data);
        // simulate a callee overwriting return data after verifier sets it
        set_return_data(&overwrite);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct VerifyCtx {}

