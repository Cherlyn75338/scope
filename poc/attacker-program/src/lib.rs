#![deny(clippy::all)]

use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
    syscalls::sol_set_return_data,
};

entrypoint!(process_instruction);

/// Instruction data layout:
///   [0..4): little-endian u32 length L
///   [4..4+L): arbitrary bytes to place into return data
pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    ix_data: &[u8],
) -> ProgramResult {
    if ix_data.len() < 4 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let len = u32::from_le_bytes(ix_data[0..4].try_into().unwrap()) as usize;
    if ix_data.len() < 4 + len {
        return Err(ProgramError::InvalidInstructionData);
    }
    let payload = &ix_data[4..4 + len];
    msg!("Attacker program setting return data len={} bytes", len);
    unsafe {
        // Safe in BPF context; syscall copies from slice
        sol_set_return_data(payload.as_ptr(), payload.len() as u64);
    }
    Ok(())
}

