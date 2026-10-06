# Daily lottery fixed rules

V1 uses lottery account layout version 2. Twelve reserved bytes now hold the frozen winner cap and absolute settlement deadline; total allocation is unchanged. This is a new program release, not an in-place upgrade or decoder for legacy accounts.

Creation snapshots the authority, fee rate, ticket price, winner cap, buy window and upload window. Checked arithmetic derives the absolute settlement deadline as upload deadline plus twice the compiled remediation duration (30 minutes). Config defaults affect only new instances. Existing-instance signatures, reveal upload, payout and rent destinations use the lottery authority snapshot. Both legacy reveal-window mutation instructions reject changes in every feature configuration. Removed source files were already excluded from instruction dispatch.

Buy and attestation windows are half-open. Omitted-reveal recovery ends at upload deadline plus the remediation duration; starting it late never extends that deadline. If finalization fails or stops partway, anyone may invoke the refund path at the absolute settlement deadline, preserving participant principal and taking no service fee. Before that deadline, the existing no-attestation/expired-omission paths remain available. A sole participant may refund after the buy window closes, without an authority transaction to change phases.

Winner entitlements vest atomically when final selection stores the final winner commitment/count/total payout. A vested lottery cannot be reinitialized or refunded. Payouts continue after the settlement deadline to the original winners, without a provider signature. Event completion and unpaid claims remain separate facts.

The default production SBF binary is exercised by the LiteSVM suite. Tests advance the clock instead of mutating live windows. Coverage includes interrupted selection, the exact timeout boundary, refund conservation, and winner payouts after timeout. Build before running runtime tests:

```sh
cargo build-sbf --manifest-path programs/daily-lottery-v1/Cargo.toml -- --locked -Znext-lockfile-bump
cargo test --locked --manifest-path programs/daily-lottery-v1/Cargo.toml
```

The mandatory registry CPI and shared daily uniqueness ledger are implemented; see `REGISTRY.md`. Finalized evidence export and public reproducible-source verification remain release gates. Build `programs/registry` first for the integrated LiteSVM tests. No new program has been deployed or made immutable by this change.
