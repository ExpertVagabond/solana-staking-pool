# solana-staking-pool

Staking pool program that distributes rewards proportionally to stakers. Supports multiple reward periods and compounding.

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Solana](https://img.shields.io/badge/Solana-9945FF?logo=solana&logoColor=white)
![Anchor](https://img.shields.io/badge/Anchor-blue)
![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)

## Features

- Proportional reward distribution
- Multiple staking periods
- Accumulated rewards tracking
- Admin reward deposits

## Program Instructions

`initialize` | `stake` | `unstake` | `claim_rewards`

## Build

```bash
anchor build
```

## Test

```bash
anchor test
```

## Deploy

```bash
# Devnet
anchor deploy --provider.cluster devnet

# Mainnet
anchor deploy --provider.cluster mainnet
```

## Project Structure

```
programs/
  solana-staking-pool/
    src/
      lib.rs          # Program entry point and instructions
    Cargo.toml
tests/
  solana-staking-pool.ts           # Integration tests
Anchor.toml             # Anchor configuration
```

## License

MIT — see [LICENSE](LICENSE) for details.

## Author

Built by [Purple Squirrel Media](https://purplesquirrelmedia.io)
