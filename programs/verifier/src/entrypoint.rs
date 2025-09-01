use solana_program::{entrypoint, pubkey::Pubkey, account_info::AccountInfo};

entrypoint!(process_instruction);

fn process_instruction(program_id: &Pubkey, accounts: &[AccountInfo], ix_data: &[u8]) -> solana_program::entrypoint::ProgramResult {
	crate::process_instruction(program_id, accounts, ix_data)
}

