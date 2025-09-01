use anchor_lang::prelude::*;
use solana_program::program::set_return_data;

declare_id!("RdSettr111111111111111111111111111111111111");

#[program]
pub mod rd_setter {
    use super::*;

    pub fn set_rd(ctx: Context<SetRd>, data: Vec<u8>) -> Result<()> {
        // Set arbitrary return data
        set_return_data(&data);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct SetRd {}

