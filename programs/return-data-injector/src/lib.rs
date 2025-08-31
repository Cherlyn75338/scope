#![allow(clippy::result_large_err)]
use solana_program::{entrypoint, entrypoint::ProgramResult, pubkey::Pubkey, account_info::AccountInfo, program_error::ProgramError, program::set_return_data};

entrypoint!(process_instruction);

pub fn process_instruction(_program_id: &Pubkey, _accounts: &[AccountInfo], _ix_data: &[u8]) -> ProgramResult {
    set_return_data(_ix_data);
    Ok(())
}

