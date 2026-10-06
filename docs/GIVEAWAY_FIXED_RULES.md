# Giveaway fixed-rule changes under development

This branch introduces giveaway account layout version 2, preserving the allocated account size by using 40 previously reserved bytes for a fixed provider authority and an absolute settlement deadline. Deploy this as a new versioned program; it is not a migration of existing program-owned accounts. Existing deployed accounts and programs remain untouched.

Creation snapshots the provider/fee authority. Attestation signatures, provider reveal publication, finalization and payout recipient validation use that snapshot. Governance changes to future defaults cannot change this event's authority. Deadlines are fixed at creation and the former extension instruction rejects changes. The absolute finalization deadline is the upload deadline plus twice the existing 30-minute remediation duration, with checked timestamp arithmetic.

Provider authority is still required for every finalization chunk. Once winners are locked, payouts can continue without the provider's signature to the validated immutable recipients. At the absolute deadline, an unvested event may refund its original creator even if finalization was partially computed; locked winners can never take this refund path. Late finalization and late winner locking are rejected. Finalization work and refund/lock races are atomic transactions.

The new state fields and authority checks have host tests and a successful SBF build. Mandatory registry creation, creator nonce uniqueness and timeout refund now have runtime coverage; see `REGISTRY.md`. Winner-vesting runtime coverage, independent audit tooling and public promotion are still required. Private reproducible builds have passed at the earlier fixed-rule commit and are rerun for each candidate. The new architecture is not production-ready and the program IDs/manifest release gate have not been activated. In particular, no immutable-upgrade-authority claim is made about current deployments.

Daily lottery runtime tests now use the default production binary and advance the LiteSVM clock against the immutable schedule. See `DAILY_LOTTERY_FIXED_RULES.md`.

## Runtime recovery evidence

Registry integration tests execute the actual SBF giveaway binary through interrupted finalization and its fixed timeout. A separate test completes winner selection, retires the release, advances past the deadline, rejects refund/reselection and pays the vested winner with only the creator signing as fee payer. The frozen provider receives the fixed fee without signing. Repeating settlement does not pay again. These tests supplement host-state tests and do not claim production deployment.
