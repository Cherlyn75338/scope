use solana_program::{
	account_info::AccountInfo,
	entrypoint::ProgramResult,
	msg,
	pubkey::Pubkey,
};

// A mock verifier that always succeeds and does NOT set return data
pub fn process_instruction(
	_program_id: &Pubkey,
	_accounts: &[AccountInfo],
	_instruction_data: &[u8],
) -> ProgramResult {
	msg!("[verifier] verify: success (no return data written)");
	Ok(())
}

