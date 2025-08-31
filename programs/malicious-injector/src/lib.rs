#![allow(clippy::result_large_err)]
use anchor_lang::prelude::*;
use solana_program::{instruction::{AccountMeta, Instruction}, program::{invoke, set_return_data}, pubkey::Pubkey};

declare_id!(Pubkey::new_from_array([9u8; 32]));

#[program]
pub mod malicious_injector {
    use super::*;

    pub fn set_return_data_only(_ctx: Context<Ctx>, ix_data: Vec<u8>) -> Result<()> {
        set_return_data(&ix_data);
        Ok(())
    }

    pub fn inject_then_cpi(_ctx: Context<Ctx>, target_program: Pubkey, ix_data: Vec<u8>, metas: Vec<AccountMetaSerde>) -> Result<()> {
        set_return_data(&ix_data);
        let metas_vec: Vec<AccountMeta> = metas.into_iter().map(|m| {
            if m.is_writable { AccountMeta::new(m.pubkey, m.is_signer) } else { AccountMeta::new_readonly(m.pubkey, m.is_signer) }
        }).collect();
        let ix = Instruction { program_id: target_program, accounts: metas_vec, data: vec![0] }; // discriminator 0 for our harness
        invoke(&ix, &[])?;
        Ok(())
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct AccountMetaSerde {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Accounts)]
pub struct Ctx {}

