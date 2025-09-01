use anchor_lang::prelude::*;
use solana_program::program::set_return_data;

declare_id!("AttaCk3rRetuRnData11111111111111111111111111");

#[program]
pub mod attacker_rd {
    use super::*;

    pub fn set_bytes(_ctx: Context<SetBytes>, data: Vec<u8>) -> Result<()> {
        set_return_data(&data);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct SetBytes {}

