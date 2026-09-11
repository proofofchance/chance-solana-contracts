# Daily Lottery Solana Contracts

This folder contains the extracted daily lottery Solana program, its helper
source, and tests.

Common local commands:

- `cargo fmt --all --check`
- `cargo test --workspace --features "allow-early-upload allow-service-charge-update"`
- `npm test`
- `npm run lint`

Production release checklist:

- [PRODUCTION_RELEASE_CHECKLIST.md](PRODUCTION_RELEASE_CHECKLIST.md)

Notes:

- Upload and attestation refer to the same participant phase.
- Local and staging builds use the `allow-early-upload` feature so operators can
  start upload without waiting for wall-clock deadlines. Production/mainnet
  builds omit that feature.
- Service charge updates are a local/staging operational feature behind
  `allow-service-charge-update`; production/mainnet builds keep the initialized
  fee immutable. Each lottery snapshots `ticket_price_lamports` and
  `service_charge_bps` at creation, so a later config change cannot alter
  already-created buy, refund, or payout math.
- Winner finalization is chunked through a `FinalizationLedger` PDA. Ticket
  purchases assign an immutable zero-based participant index and cap a round at
  4,096 unique participants. Any caller may submit chunks, but every pass must
  consume the exact next index.
- Only verified reveal-included participants are eligible. The canonical pool
  commitment binds index, wallet, ticket count, and reveal digest; draw indices
  use rejection sampling.
- Winner finalization rejects any selected winner set that would pay less than
  one lamport per winner.
- Lottery accounts include an explicit layout version and reserved bytes so
  future controlled upgrades can add fields without immediately changing the
  serialized account size.
- Multiple lotteries may be active concurrently; operational commands should target
  an explicit lottery ID when settling or paying winners.
- Single-participant lotteries auto-complete upload when the upload/attestation
  phase opens and can settle immediately through refund semantics. Multi-participant
  no-attester or zero-reveal refunds remain gated on the upload deadline.
- After winners are finalized, `SettlePayoutBatch` is permissionless. The caller
  cannot change recipients or amounts because each payout must match its fixed
  winner-page PDA; service fee, remainder, and vault rent still go to the
  configured authority account.
- Refund/cancel rounds leave participant funds in the program vault until each
  participant claims with `ClaimRefund`. After all refund claims are recorded,
  anyone may call `CloseRefundVault` to return the vault rent to the configured
  authority account.
- After a terminal outcome, participants can call `CloseParticipant` to reclaim
  their participant PDA rent. Refund-path closes require that wallet's refund to
  be claimed first; winner-path closes require payout settlement to be complete.
- Participants can bypass provider attestation receipts by submitting
  `AttestReveal`, which verifies their reveal against the original commitment and
  includes it in settlement entropy and the ticket-weighted winner-count tally.
- Each winner-page append emits `WinnerSelected`, so event replay can rebuild the
  winner set after participant accounts are closed.
