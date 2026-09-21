# Contributing to NovaEvents

Thanks for considering a contribution. This guide covers everything you need to go from zero to a merged PR.

## What this project is

NovaEvents is a Soroban smart contract (Rust) for transparent event management on Stellar. Every financial action — sponsor contributions, ticket sales, and worker payouts — settles on-chain through a single contract that serves as the public ledger for an event.

If you haven't read the README, do that first. It explains the roles, the transparency thesis, and the overall architecture.

## Before you start

Browse the open issues. Each issue has clear acceptance criteria that define what "done" looks like. Pick one, leave a comment so others know it's being worked on, and only then start writing code.

If you want to work on something that isn't in the issues, open an issue first and describe what you'd like to build. Don't spend time writing code for a change that hasn't been discussed.

## Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable)
- The `wasm32v1-none` target:
  ```bash
  rustup target add wasm32v1-none
  ```
- [Stellar CLI](https://developers.stellar.org/docs/build/smart-contracts/getting-started/setup) for deploying to testnet

### Format

```bash
cargo fmt
```

### Lint

```bash
cargo clippy -- -D warnings
```

### Build

```bash
cargo build
```

### Test

```bash
cargo test
```

All 18 tests must pass before you open a PR. New functions require new tests.

### Build the WASM artifact

```bash
cargo build --target wasm32v1-none --release
```

The compiled contract ends up at `target/wasm32v1-none/release/nova_events.wasm`.

### Deploy to testnet (optional, for manual verification)

```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/nova_events.wasm \
  --network testnet \
  --source <your-account>
```

## End-to-end lifecycle verification

`cargo test` runs the contract in-process against `Env::default()`. That catches
logic bugs, but it never serializes arguments over XDR, never signs a real
transaction, and never crosses a contract boundary into the token — so it cannot
catch deployment- or ABI-level breakage.

`scripts/e2e_lifecycle.sh` covers that gap. It drives a freshly deployed contract
through a full event lifecycle using only the Stellar CLI and RPC:

```
deploy -> initialize -> create_event -> buy_ticket -> transfer_ticket
       -> redeem_ticket -> sponsor_event -> end_event -> payout
```

At every stage it asserts both the happy path and the expected `#[contracterror]`
code for the matching failure, and it cross-checks the event balance the contract
reports against the token contract's own view of what it holds.

```bash
# Local Soroban sandbox — self-contained, requires a running Docker daemon
./scripts/e2e_lifecycle.sh

# Public testnet — no Docker, needs internet
./scripts/e2e_lifecycle.sh --network testnet
```

| Option | Effect |
|--------|--------|
| `--network <local\|testnet>` | Target network. Default `local`. |
| `--skip-build` | Reuse the existing WASM instead of rebuilding. |
| `--keep-sandbox` | Leave the local container running after the run. |

The script generates and funds four throwaway accounts (organizer, attendee,
sponsor, worker) and removes them on exit. It settles in the native Stellar Asset
Contract instead of USDC, so no issuer, trustline, or minting setup is needed —
the contract stores whatever token address `initialize` is given, so the flow is
identical either way.

**Run this before tagging a release**, and after any change to a public
entrypoint's signature. It is deliberately not part of CI: it needs either Docker
or live network access, and it submits real transactions.

The script exercises the full lifecycle, including `end_event -> payout`:
after ending the event it confirms ticket sales and sponsorships are locked,
then disburses a payout and verifies the recipient actually received the
funds.

## Making a contribution

1. Fork the repository and create a branch named after the issue: `issue-42-ticket-transfer`.
2. Write your code. Keep changes focused on the issue — don't refactor unrelated things in the same PR.
3. Add or update tests. Every new function needs a test that covers the happy path and at least one failure case.
4. Run `cargo test` and make sure everything passes. If you changed a public
   entrypoint's signature, also run `scripts/e2e_lifecycle.sh --network testnet`.
5. Open a pull request against `main`. Fill in the PR description: what changed, why, and how you tested it.

## Code standards

- This is a `no_std` Soroban contract. Don't introduce `std`-only dependencies.
- Storage: use `persistent` for per-event data, `instance` for contract-wide config.
- Auth: any function that changes state on behalf of a user must call `address.require_auth()` at the top.
- Error handling: `panic!` with a short descriptive string is fine for contract-level invariant violations.
- Comments: only add a comment when the *why* is non-obvious. Don't describe what the code does — the code does that.

## AI-assisted contributions

You may use AI tools to help write or understand code. However:

- You are responsible for every line you submit. Review and understand all AI-generated code before including it in a PR.
- Submitting unreviewed AI output — code you can't explain or defend — is grounds for a flag under the GrantFox quality policy.
- If a reviewer asks you to explain a section of your PR, you should be able to do so.

## Amounts and precision

USDC on Stellar uses 7 decimal places. `1 USDC = 10_000_000 stroops`. Use stroops (`i128`) everywhere inside the contract; convert only at the boundary where you display or input values.

## Questions

Open an issue or leave a comment on the relevant issue thread. Don't open a PR without prior discussion for anything beyond a small, clearly scoped bug fix.
