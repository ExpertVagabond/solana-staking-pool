# solana-staking-pool

Reward-bearing SPL token staking pool with time-weighted accumulated rewards on Solana.

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust) ![Solana](https://img.shields.io/badge/Solana-9945FF?logo=solana&logoColor=white) ![Anchor](https://img.shields.io/badge/Anchor-blue) ![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

## Overview

A Solana Anchor program implementing a staking pool where users deposit SPL tokens and earn rewards proportional to their share of the pool over time. Rewards are calculated using a `reward_per_token` accumulator pattern with 18-decimal precision (`1e18`), ensuring fair distribution across all stakers regardless of entry time.

## Program Instructions

| Instruction | Description | Key Accounts |
|---|---|---|
| `initialize_pool` | Create a staking pool for a stake/reward mint pair with a configurable reward rate | `authority` (signer), `stake_mint`, `reward_mint`, `pool` (PDA), `stake_vault`, `reward_vault` |
| `stake` | Deposit tokens into the pool and auto-harvest pending rewards | `owner` (signer), `pool`, `stake_entry` (PDA), `stake_vault`, `reward_vault`, `user_stake_ata`, `user_reward_ata` |
| `unstake` | Withdraw staked tokens and harvest pending rewards | `owner` (signer), `pool`, `stake_entry`, `stake_vault`, `reward_vault`, `user_stake_ata`, `user_reward_ata` |
| `claim_rewards` | Harvest accumulated rewards without unstaking | `owner` (signer), `pool`, `stake_entry`, `reward_vault`, `user_reward_ata` |
| `update_reward_rate` | Admin-only: change the reward emission rate | `authority` (signer), `pool` |

## Account Structures

### StakePool

| Field | Type | Description |
|---|---|---|
| `authority` | `Pubkey` | Pool admin |
| `stake_mint` | `Pubkey` | Token users deposit |
| `reward_mint` | `Pubkey` | Token distributed as rewards |
| `total_staked` | `u64` | Total tokens currently staked |
| `reward_rate` | `u64` | Rewards per second (in raw token units) |
| `last_update_ts` | `i64` | Last reward accumulation timestamp |
| `accumulated_reward_per_token` | `u128` | Cumulative reward per token (18-decimal precision) |
| `bump` | `u8` | PDA bump seed |

### StakeEntry

| Field | Type | Description |
|---|---|---|
| `owner` | `Pubkey` | Staker's wallet |
| `pool` | `Pubkey` | Associated pool |
| `amount` | `u64` | Tokens staked by this user |
| `reward_debt` | `u128` | Reward debt for pending calculation |
| `bump` | `u8` | PDA bump seed |

## PDA Seeds

- **Pool:** `["pool", authority, stake_mint]`
- **StakeEntry:** `["stake", pool, owner]`

## Reward Math

Rewards use a time-weighted accumulator:

```
accumulated_reward_per_token += (elapsed_seconds * reward_rate * 1e18) / total_staked
pending_reward = (user_amount * accumulated_reward_per_token - reward_debt) / 1e18
```

## Error Codes

| Error | Description |
|---|---|
| `ZeroAmount` | Stake/unstake amount must be greater than zero |
| `MathOverflow` | Arithmetic overflow in reward calculation |
| `InsufficientStake` | Cannot unstake more than deposited |
| `NoPendingRewards` | No rewards available to claim |

## Build & Test

```bash
anchor build
anchor test
```

## Deploy

```bash
solana config set --url devnet
anchor deploy
```

## License

[MIT](LICENSE)
