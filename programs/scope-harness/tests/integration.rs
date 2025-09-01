use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{signer::Signer, transaction::Transaction};

#[tokio::test]
async fn test_injector_wins_when_verifier_no_return_data() {
    // Programs
    let harness_id = scope_harness::id();
    let injector_id = Pubkey::new_from_array([9u8;32]);
    let verifier_id = verifier_stub::id();

    let mut pt = ProgramTest::new(
        "scope_harness",
        harness_id,
        processor!(scope_harness::entry),
    );
    pt.add_program("malicious_injector", injector_id, processor!(malicious_injector::entry));
    pt.add_program("verifier_stub", verifier_id, processor!(verifier_stub::entry));

    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    // Attacker-chosen non-empty bytes
    let attacker_bytes = vec![1,2,3,4,5];

    // Build malicious injector ix which sets return data to attacker_bytes and then CPIs to harness
    let ix_injector = Instruction {
        program_id: injector_id,
        accounts: vec![],
        data: malicious_injector::instruction::InjectThenCpi { target_program: harness_id, ix_data: attacker_bytes.clone(), metas: vec![] }.data(),
    };

    // Build harness ix (will be invoked via injector CPI)
    let tx = Transaction::new_signed_with_payer(&[ix_injector], Some(&payer.pubkey()), &[&payer], recent_blockhash);

    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "tx should succeed; harness decodes attacker bytes");
}

#[tokio::test]
async fn test_verifier_overwrite_still_allows_attacker_if_last_writer_not_verifier() {
    // Here we simulate verifier setting data then another program overwriting, but since our harness is invoked only once
    // we replicate by calling injector to set attacker bytes just before harness.
    let harness_id = scope_harness::id();
    let injector_id = Pubkey::new_from_array([9u8;32]);

    let mut pt = ProgramTest::new("scope_harness", harness_id, processor!(scope_harness::entry));
    pt.add_program("malicious_injector", injector_id, processor!(malicious_injector::entry));

    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    let attacker_bytes = vec![9,9,9];

    let ix_injector = Instruction {
        program_id: injector_id,
        accounts: vec![],
        data: malicious_injector::instruction::SetReturnDataOnly { ix_data: attacker_bytes }.data(),
    };
    let ix_harness = Instruction { program_id: harness_id, accounts: vec![], data: vec![0] };

    let tx = Transaction::new_signed_with_payer(&[ix_injector, ix_harness], Some(&payer.pubkey()), &[&payer], recent_blockhash);
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "attacker data was last-writer before harness; harness accepted");
}

#[tokio::test]
async fn test_last_writer_overwrites_verifier_data() {
    // Simulate: verifier sets return data, but a later program overwrites it, then harness runs
    let harness_id = scope_harness::id();
    let injector_id = Pubkey::new_from_array([9u8;32]);
    let verifier_id = verifier_stub::id();

    let mut pt = ProgramTest::new("scope_harness", harness_id, processor!(scope_harness::entry));
    pt.add_program("malicious_injector", injector_id, processor!(malicious_injector::entry));
    pt.add_program("verifier_stub", verifier_id, processor!(verifier_stub::entry));

    let (mut banks_client, payer, recent_blockhash) = pt.start().await;

    // Step 1: call verifier that sets return data to some genuine-looking bytes
    let verifier_bytes = vec![0xAA; 16];
    let ix_verifier = Instruction {
        program_id: verifier_id,
        accounts: vec![],
        data: verifier_stub::instruction::VerifyWithReturnData { data: verifier_bytes }.data(),
    };

    // Step 2: attacker overwrites return data
    let attacker_bytes = vec![7,7,7,7,7];
    let ix_overwrite = Instruction {
        program_id: injector_id,
        accounts: vec![],
        data: malicious_injector::instruction::SetReturnDataOnly { ix_data: attacker_bytes.clone() }.data(),
    };

    // Step 3: harness reads last-writer data (attacker)
    let ix_harness = Instruction { program_id: harness_id, accounts: vec![], data: vec![0] };

    let tx = Transaction::new_signed_with_payer(&[ix_verifier, ix_overwrite, ix_harness], Some(&payer.pubkey()), &[&payer], recent_blockhash);
    let res = banks_client.process_transaction(tx).await;
    assert!(res.is_ok(), "attacker overwrote after verifier; harness accepted attacker bytes");
}

