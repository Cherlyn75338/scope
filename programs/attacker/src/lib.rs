use solana_program::{
	account_info::AccountInfo,
	entrypoint::ProgramResult,
	msg,
	program::set_return_data,
	pubkey::Pubkey,
};

pub fn process_instruction(
	_program_id: &Pubkey,
	_accounts: &[AccountInfo],
	instruction_data: &[u8],
) -> ProgramResult {
	// Echo instruction_data into transaction return data
	set_return_data(instruction_data);
	msg!("[attacker] set_return_data: {} bytes", instruction_data.len());
	Ok(())
}

