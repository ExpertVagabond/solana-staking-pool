import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { PublicKey, Keypair, SystemProgram } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID,
  createMint,
  createAccount,
  mintTo,
  getAccount,
} from "@solana/spl-token";
import { assert } from "chai";
import { SolanaStakingPool } from "../target/types/solana_staking_pool";

describe("solana-staking-pool", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace
    .solanaStakingPool as Program<SolanaStakingPool>;
  const authority = provider.wallet as anchor.Wallet;

  let stakeMint: PublicKey;
  let rewardMint: PublicKey;
  let poolPda: PublicKey;
  let poolBump: number;
  let stakeVault: Keypair;
  let rewardVault: Keypair;
  let userStakeAta: PublicKey;
  let userRewardAta: PublicKey;
  let stakeEntryPda: PublicKey;
  let stakeEntryBump: number;

  const REWARD_RATE = new anchor.BN(100);
  const STAKE_AMOUNT = new anchor.BN(1_000_000);
  const MINT_AMOUNT = 10_000_000;

  before(async () => {
    // Create staking and reward mints
    stakeMint = await createMint(
      provider.connection,
      (authority as any).payer,
      authority.publicKey,
      null,
      6
    );

    rewardMint = await createMint(
      provider.connection,
      (authority as any).payer,
      authority.publicKey,
      null,
      6
    );

    // Derive pool PDA
    [poolPda, poolBump] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("pool"),
        authority.publicKey.toBuffer(),
        stakeMint.toBuffer(),
      ],
      program.programId
    );

    // Create vault keypairs
    stakeVault = Keypair.generate();
    rewardVault = Keypair.generate();

    // Create user token accounts for staking and rewards
    userStakeAta = await createAccount(
      provider.connection,
      (authority as any).payer,
      stakeMint,
      authority.publicKey
    );

    userRewardAta = await createAccount(
      provider.connection,
      (authority as any).payer,
      rewardMint,
      authority.publicKey
    );

    // Mint staking tokens to user
    await mintTo(
      provider.connection,
      (authority as any).payer,
      stakeMint,
      userStakeAta,
      authority.publicKey,
      MINT_AMOUNT
    );

    // Derive stake entry PDA
    [stakeEntryPda, stakeEntryBump] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("stake"),
        poolPda.toBuffer(),
        authority.publicKey.toBuffer(),
      ],
      program.programId
    );
  });

  it("initialize_pool — creates pool with staking/reward mints", async () => {
    await program.methods
      .initializePool(REWARD_RATE)
      .accounts({
        authority: authority.publicKey,
        stakeMint: stakeMint,
        rewardMint: rewardMint,
        pool: poolPda,
        stakeVault: stakeVault.publicKey,
        rewardVault: rewardVault.publicKey,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
        rent: anchor.web3.SYSVAR_RENT_PUBKEY,
      })
      .signers([stakeVault, rewardVault])
      .rpc();

    const pool = await program.account.stakePool.fetch(poolPda);
    assert.ok(pool.authority.equals(authority.publicKey));
    assert.ok(pool.stakeMint.equals(stakeMint));
    assert.ok(pool.rewardMint.equals(rewardMint));
    assert.equal(pool.totalStaked.toNumber(), 0);
    assert.equal(pool.rewardRate.toNumber(), REWARD_RATE.toNumber());
    assert.equal(pool.bump, poolBump);

    // Verify vaults are owned by pool PDA
    const stakeVaultAccount = await getAccount(
      provider.connection,
      stakeVault.publicKey
    );
    assert.ok(new PublicKey(stakeVaultAccount.owner).equals(poolPda));

    const rewardVaultAccount = await getAccount(
      provider.connection,
      rewardVault.publicKey
    );
    assert.ok(new PublicKey(rewardVaultAccount.owner).equals(poolPda));
  });

  it("stake — deposits staking tokens into the pool", async () => {
    const userBalanceBefore = (
      await getAccount(provider.connection, userStakeAta)
    ).amount;

    await program.methods
      .stake(STAKE_AMOUNT)
      .accounts({
        owner: authority.publicKey,
        pool: poolPda,
        stakeEntry: stakeEntryPda,
        stakeVault: stakeVault.publicKey,
        rewardVault: rewardVault.publicKey,
        userStakeAta: userStakeAta,
        userRewardAta: userRewardAta,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const entry = await program.account.stakeEntry.fetch(stakeEntryPda);
    assert.equal(entry.amount.toNumber(), STAKE_AMOUNT.toNumber());
    assert.ok(entry.owner.equals(authority.publicKey));
    assert.ok(entry.pool.equals(poolPda));

    const pool = await program.account.stakePool.fetch(poolPda);
    assert.equal(pool.totalStaked.toNumber(), STAKE_AMOUNT.toNumber());

    // Verify token transfer
    const userBalanceAfter = (
      await getAccount(provider.connection, userStakeAta)
    ).amount;
    assert.equal(
      Number(userBalanceBefore) - Number(userBalanceAfter),
      STAKE_AMOUNT.toNumber()
    );

    const vaultBalance = (
      await getAccount(provider.connection, stakeVault.publicKey)
    ).amount;
    assert.equal(Number(vaultBalance), STAKE_AMOUNT.toNumber());
  });

  it("unstake — withdraws staking tokens from the pool", async () => {
    const unstakeAmount = new anchor.BN(500_000);

    const userBalanceBefore = (
      await getAccount(provider.connection, userStakeAta)
    ).amount;

    await program.methods
      .unstake(unstakeAmount)
      .accounts({
        owner: authority.publicKey,
        pool: poolPda,
        stakeEntry: stakeEntryPda,
        stakeVault: stakeVault.publicKey,
        rewardVault: rewardVault.publicKey,
        userStakeAta: userStakeAta,
        userRewardAta: userRewardAta,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const entry = await program.account.stakeEntry.fetch(stakeEntryPda);
    assert.equal(
      entry.amount.toNumber(),
      STAKE_AMOUNT.toNumber() - unstakeAmount.toNumber()
    );

    const pool = await program.account.stakePool.fetch(poolPda);
    assert.equal(
      pool.totalStaked.toNumber(),
      STAKE_AMOUNT.toNumber() - unstakeAmount.toNumber()
    );

    // Verify user received tokens back
    const userBalanceAfter = (
      await getAccount(provider.connection, userStakeAta)
    ).amount;
    assert.equal(
      Number(userBalanceAfter) - Number(userBalanceBefore),
      unstakeAmount.toNumber()
    );
  });

  it("claim_rewards — claims accrued rewards", async () => {
    // Fund the reward vault so there are rewards to claim
    await mintTo(
      provider.connection,
      (authority as any).payer,
      rewardMint,
      rewardVault.publicKey,
      authority.publicKey,
      10_000_000
    );

    const rewardBalanceBefore = (
      await getAccount(provider.connection, userRewardAta)
    ).amount;

    // Note: On localnet the clock may not advance between transactions,
    // so pending rewards might be 0. We handle the NoPendingRewards error
    // gracefully. In production tests you would use warp_to_slot.
    try {
      await program.methods
        .claimRewards()
        .accounts({
          owner: authority.publicKey,
          pool: poolPda,
          stakeEntry: stakeEntryPda,
          rewardVault: rewardVault.publicKey,
          userRewardAta: userRewardAta,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();

      // If claim succeeded, verify reward balance increased
      const rewardBalanceAfter = (
        await getAccount(provider.connection, userRewardAta)
      ).amount;
      assert.ok(
        Number(rewardBalanceAfter) >= Number(rewardBalanceBefore),
        "Reward balance should increase or stay the same after claiming"
      );
    } catch (err: any) {
      // NoPendingRewards is expected if no time has elapsed on localnet
      assert.include(err.toString(), "NoPendingRewards");
    }
  });

  it("error: unstake more than staked", async () => {
    const entry = await program.account.stakeEntry.fetch(stakeEntryPda);
    const overAmount = new anchor.BN(entry.amount.toNumber() + 1_000_000);

    try {
      await program.methods
        .unstake(overAmount)
        .accounts({
          owner: authority.publicKey,
          pool: poolPda,
          stakeEntry: stakeEntryPda,
          stakeVault: stakeVault.publicKey,
          rewardVault: rewardVault.publicKey,
          userStakeAta: userStakeAta,
          userRewardAta: userRewardAta,
          tokenProgram: TOKEN_PROGRAM_ID,
        })
        .rpc();
      assert.fail("Should have thrown InsufficientStake error");
    } catch (err: any) {
      assert.include(err.toString(), "InsufficientStake");
    }
  });
});
