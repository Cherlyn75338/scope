use solana_program::{
    account_info::AccountInfo,
    entrypoint,
    entrypoint::ProgramResult,
    instruction::Instruction,
    log::sol_log,
    program::{get_return_data, invoke, set_return_data},
    program_error::ProgramError,
    pubkey::Pubkey,
};

entrypoint!(process_instruction);

pub const WRITER_ID: Pubkey = Pubkey::new_from_array([1u8; 32]);
pub const VERIFIER_ID: Pubkey = Pubkey::new_from_array([2u8; 32]);
pub const SCOPE_ID: Pubkey = Pubkey::new_from_array([3u8; 32]);

pub fn process_instruction(
    program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if *program_id == WRITER_ID {
        match instruction_data {
            b"attacker" => {
                set_return_data(b"ATTACKER");
                sol_log("writer(attacker) done");
                Ok(())
            }
            b"overwriter" => {
                set_return_data(b"OVERWRITER");
                sol_log("writer(overwriter) done");
                Ok(())
            }
            _ => Err(ProgramError::InvalidInstructionData),
        }
    } else if *program_id == VERIFIER_ID {
        match instruction_data {
            b"verify" => {
                set_return_data(b"VERIFIER");
                sol_log("verifier done");
                Ok(())
            }
            _ => Err(ProgramError::InvalidInstructionData),
        }
    } else if *program_id == SCOPE_ID {
        match instruction_data {
            b"scope_ro" => {
                // Read whatever the last writer set, without CPI
                if let Some((pid, data)) = get_return_data() {
                    sol_log(&format!(
                        "scope_ro got pid={} data={}",
                        pid,
                        core::str::from_utf8(&data).unwrap_or("<nonutf8>")
                    ));
                    if pid != WRITER_ID { return Err(ProgramError::Custom(20)); }
                    Ok(())
                } else {
                    return Err(ProgramError::Custom(21));
                }
            }
            b"scope_cpi_a" => {
                // accounts[0] = writer program account, accounts[1] = verifier program account
                let writer_ai = &_accounts[0];
                let verifier_ai = &_accounts[1];
                // CPI: writer -> verifier -> read (should observe verifier)
                invoke(
                    &Instruction { program_id: WRITER_ID, accounts: vec![], data: b"attacker".to_vec() },
                    &[writer_ai.clone()],
                )?;
                invoke(
                    &Instruction { program_id: VERIFIER_ID, accounts: vec![], data: b"verify".to_vec() },
                    &[verifier_ai.clone()],
                )?;
                if let Some((pid, data)) = get_return_data() {
                    sol_log(&format!("scope_cpi_a got pid={} data={}", pid, core::str::from_utf8(&data).unwrap_or("<nonutf8>")));
                    if pid != VERIFIER_ID { return Err(ProgramError::Custom(10)); }
                    Ok(())
                } else {
                    return Err(ProgramError::Custom(11));
                }
            }
            b"scope_cpi_b" => {
                // accounts[0] = writer program account, accounts[1] = verifier program account
                let writer_ai = &_accounts[0];
                let verifier_ai = &_accounts[1];
                // CPI: writer -> verifier -> writer(overwrite) -> read (should observe writer)
                invoke(
                    &Instruction { program_id: WRITER_ID, accounts: vec![], data: b"attacker".to_vec() },
                    &[writer_ai.clone()],
                )?;
                invoke(
                    &Instruction { program_id: VERIFIER_ID, accounts: vec![], data: b"verify".to_vec() },
                    &[verifier_ai.clone()],
                )?;
                invoke(
                    &Instruction { program_id: WRITER_ID, accounts: vec![], data: b"overwriter".to_vec() },
                    &[writer_ai.clone()],
                )?;
                if let Some((pid, data)) = get_return_data() {
                    sol_log(&format!("scope_cpi_b got pid={} data={}", pid, core::str::from_utf8(&data).unwrap_or("<nonutf8>")));
                    if pid != WRITER_ID { return Err(ProgramError::Custom(12)); }
                    Ok(())
                } else {
                    return Err(ProgramError::Custom(13));
                }
            }
            _ => Err(ProgramError::InvalidInstructionData),
        }
    } else {
        Err(ProgramError::IncorrectProgramId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program_test::{processor, ProgramTest, ProgramTestBanksClientExt};
    use solana_sdk::{signer::Signer, transaction::Transaction};

    #[tokio::test]
    async fn last_writer_wins() {
        let mut pt = ProgramTest::default();
        pt.add_program("writer_prog", WRITER_ID, processor!(process_instruction));
        pt.add_program("verifier_prog", VERIFIER_ID, processor!(process_instruction));
        pt.add_program("scope_prog", SCOPE_ID, processor!(process_instruction));

        let (mut banks_client, payer, recent_blockhash) = pt.start().await;

        // Scenario A (via CPI inside scope): writer -> verifier -> scope reads
        let ix_scope_a = solana_sdk::instruction::Instruction {
            program_id: SCOPE_ID,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new_readonly(WRITER_ID, false),
                solana_sdk::instruction::AccountMeta::new_readonly(VERIFIER_ID, false),
            ],
            data: b"scope_cpi_a".to_vec(),
        };
        let tx_a = Transaction::new_signed_with_payer(
            &[ix_scope_a],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash,
        );
        banks_client.process_transaction(tx_a).await.expect("scope_a should succeed");

        // Scenario B (via CPI inside scope): writer -> verifier -> writer(overwriter) -> scope reads
        let recent_blockhash2 = banks_client.get_latest_blockhash().await.unwrap();
        let ix_scope_b = solana_sdk::instruction::Instruction {
            program_id: SCOPE_ID,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new_readonly(WRITER_ID, false),
                solana_sdk::instruction::AccountMeta::new_readonly(VERIFIER_ID, false),
            ],
            data: b"scope_cpi_b".to_vec(),
        };
        let tx_b = Transaction::new_signed_with_payer(
            &[ix_scope_b],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash2,
        );
        banks_client.process_transaction(tx_b).await.expect("scope_b should succeed");

        // Scenario C (unguarded preceding instruction): attacker sets data in a prior ix; scope reads without CPI
        let recent_blockhash3 = banks_client.get_latest_blockhash().await.unwrap();
        let ix_writer_pre = solana_sdk::instruction::Instruction { program_id: WRITER_ID, accounts: vec![], data: b"attacker".to_vec() };
        let ix_scope_ro = solana_sdk::instruction::Instruction { program_id: SCOPE_ID, accounts: vec![], data: b"scope_ro".to_vec() };
        let tx_c = Transaction::new_signed_with_payer(
            &[ix_writer_pre, ix_scope_ro],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash3,
        );
        banks_client.process_transaction(tx_c).await.expect("scope_ro should succeed and observe writer");

        // Scenario D (preceding attacker but verifier overwrites inside scope): still safe
        let recent_blockhash4 = banks_client.get_latest_blockhash().await.unwrap();
        let ix_writer_pre2 = solana_sdk::instruction::Instruction { program_id: WRITER_ID, accounts: vec![], data: b"attacker".to_vec() };
        let ix_scope_a2 = solana_sdk::instruction::Instruction {
            program_id: SCOPE_ID,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new_readonly(WRITER_ID, false),
                solana_sdk::instruction::AccountMeta::new_readonly(VERIFIER_ID, false),
            ],
            data: b"scope_cpi_a".to_vec(),
        };
        let tx_d = Transaction::new_signed_with_payer(
            &[ix_writer_pre2, ix_scope_a2],
            Some(&payer.pubkey()),
            &[&payer],
            recent_blockhash4,
        );
        banks_client.process_transaction(tx_d).await.expect("preceding writer should be overwritten by verifier inside scope");
    }
}
