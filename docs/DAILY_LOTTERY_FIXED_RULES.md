# Daily lottery fixed rules

V1 uses lottery account layout version 2. Twelve reserved bytes now hold the frozen winner cap and absolute settlement deadline; total allocation is unchanged. This is a new program release, not an in-place upgrade or decoder for legacy accounts.

Creation snapshots the authority, fee rate, ticket price, winner cap, buy window and upload window. Checked arithmetic derives the absolute settlement deadline as upload deadline plus twice the compiled remediation duration (30 minutes). Config defaults affect only new instances. Existing-instance signatures, reveal upload, payout and rent destinations use the lottery authority snapshot. Default and explicit production builds reject both legacy timing instructions. The duration-bearing editor remains disabled in every build; explicitly bound local/staging testing artifacts can enable only the single-use begin-upload instruction described below. Removed source files were already excluded from instruction dispatch.

Buy and attestation windows are half-open. Omitted-reveal recovery ends at upload deadline plus the remediation duration; starting it late never extends that deadline. If finalization fails or stops partway, anyone may invoke the refund path at the absolute settlement deadline, preserving participant principal and taking no service fee. Before that deadline, the existing no-attestation/expired-omission paths remain available. A sole participant may refund after the buy window closes, without an authority transaction to change phases.

Winner entitlements vest atomically when final selection stores the final winner commitment/count/total payout. A vested lottery cannot be reinitialized or refunded. Payouts continue after the settlement deadline to the original winners, without a provider signature. Event completion and unpaid claims remain separate facts.

The default production SBF binary is exercised by the LiteSVM suite. Tests advance the clock instead of mutating live windows. Coverage includes interrupted selection, the exact timeout boundary, refund conservation, and winner payouts after timeout. Build before running runtime tests:

```sh
cargo build-sbf --manifest-path programs/daily-lottery-v1/Cargo.toml -- --locked -Znext-lockfile-bump
cargo test --locked --manifest-path programs/daily-lottery-v1/Cargo.toml
```

The mandatory registry CPI and shared daily uniqueness ledger are implemented; see `REGISTRY.md`. Finalized evidence export and public reproducible-source verification remain release gates. Build `programs/registry` first for the integrated LiteSVM tests. No new program has been deployed or made immutable by this change.

## Non-production begin-upload

Business version stays V1 and lottery account layout stays version 2. The
`allow-early-upload` capability requires exactly one explicit scope:
`local-test-controls` or `staging-test-controls`. Staging also requires
`staging-afresh-v1`. `production-release` rejects all testing scopes, staging
identity inputs and mutable-fee testing features. Both the Cargo build script and
actual Rust cfg enforce the combinations, including injected cfg flags.

Local builds read canonical public program/registry/authority identities from
ignored `releases/local-test-controls-v1.inputs`; staging uses the existing
per-epoch public inputs. The bound original authority is required. Tag 8 takes no
timestamps or durations, requires six canonical original-release accounts and
moves an active buying round to upload once. It preserves the frozen upload
duration and relative remediation/settlement deadlines. Scheduled, already
uploading, attested, finalizing, refunding and terminal states reject. Four
effective timing fields change and bit 0 of `Lottery.reserved[0]` is set as a
durable testing-only one-shot marker. Even a backwards clock cannot reopen
buying or reuse the shortcut. Business key, original participation start,
participants, commitments and balances remain unchanged. The complete transition
emits testing-only `TestUploadPhaseBegan`. Tag 6 always rejects.

Read-only tag 21 returns the executing artifact's schema, business version,
scope, capability bits and compiled program/registry/authority bindings. Its
fixed-width wire contract is `audit/schemas/daily-test-controls-v1.json`.
Production/default artifacts report zero testing powers. Clients must verify the
immutable original program, registry and genesis, then execute this query and
simulate tag 8 before signing. A profile label cannot enable the capability.

`tools/test_non_production_controls.py` tests feature composition, strict public
inputs and the actual rustc cfg. Its `--write-fixtures` mode creates only public
isolated runtime fixtures and refuses to replace different operational inputs.
Those known-key fixtures are qualification inputs, never live release identities.
The `Daily lottery testing controls` CI workflow reproduces production, local
and staging flavors independently and executes their actual SBF artifacts.
Production runs the complete naturally timed suite; enabled flavors test the
single-use cutoff, zero/one/two buyers, authority/origin failures, conserved state
and rejected repeat calls. Qualification artifacts do not deploy or admit a new
staging release. Private/public byte comparison and a qualified fresh epoch remain
separate deployment gates.
