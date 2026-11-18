# Chance Solana contracts

Public source and verification tools for the versioned Chance registry, daily lotteries and giveaways. Each event has its own account and vault; new releases govern future creation. Existing events retain their original rules, settlement and refund rights.

Giveaway creation is permissionless through an active immutable release. New
versions use new addresses; the future creation catalog can choose a default
without changing existing instances. See [release selection](docs/GIVEAWAY_RELEASE_SELECTION.md).

## Independent build roots

| Library | Manifest | Purpose |
|---|---|---|
| `chance_registry` | `programs/registry/Cargo.toml` | Release lifecycle, reasons, successors and instance inventory |
| `daily_lottery` | `programs/daily-lottery-v1/Cargo.toml` | Daily event creation, weighted selection, payouts and refunds |
| `giveaways` | `programs/giveaways-v1/Cargo.toml` | Creator events, ranked selection, payouts and refunds |

These are separate Cargo workspaces with pinned locks. Shared build inputs live in `crates/`; builds require no sibling repositories. The root Cargo.lock symlink supports verifier discovery without unifying dependency versions.

Follow [reproducible builds](docs/REPRODUCIBLE_BUILDS.md) for the pinned verifier/container and each selected library. CI compares two clean builds per executable and runs tests against those exact binaries. [Registry](docs/REGISTRY.md), [daily rules](docs/DAILY_LOTTERY_FIXED_RULES.md) and [giveaway rules](docs/GIVEAWAY_FIXED_RULES.md) describe protocol behavior. [Audit tools](audit/README.md) distinguish source/binary equality, authority checks, execution replay and security review.

## Current dependency candidate

Production program APIs use Solana 5.2.0; giveaways uses stable Anchor 1.2.2 with
its `solana-v4` compatibility feature. Host checks use Rust 1.100.0. The pinned
verifier is 0.5.2 and its current official supported builder uses platform-tools
1.57. The separately prepared [latest builder recipe](tools/sbf-builder/README.md)
uses SHA-verified tool 4.6.0/platform-tools 1.58; Docker reproduction remains
qualification-pending after local image-download truncation. No image digest or
existing deployment is relabeled.

Run host invariants with `cargo test --locked --lib --manifest-path
programs/<domain>/Cargo.toml`. The independent `tests/runtime/Cargo.toml` harness
runs the original daily and registry suites against actual production-built ELF
files. Set `SBF_OUT_DIR` to their deploy directory. Latest stable LiteSVM 0.18.2
pins an older compatible API generation; its shared-source host model never
supplies a program ELF. The production lockfiles remain independent.

Production locks report no current vulnerabilities and retain the upstream
bincode maintenance warning. The isolated simulator has three known upstream
vulnerabilities and six informational findings. On 2026-11-17 the user approved
only the [exact runtime baseline](audit/runtime-advisory-baseline.json), lock and
62 reviewed harness/test/model/feature source hashes. Reviewed precompile paths
verify public inputs; the old signing/secret-arithmetic helpers are not called,
and no production signing keys enter the harness. New or changed findings,
versions, categories, advisory details or source context fail closed. Complete
raw reports and the original scanner exit remain visible; production and tool
locks retain strict audits. Compatible patched upstream precompiles remain the
remediation. The [reviewed scanner bootstrap](tools/dependency-audit/README.md)
self-audits the patched cargo-audit 0.23.0 build without ignore flags.

## Non-production daily upload controls

Daily lottery V1 keeps fixed production timing. Explicit local/staging builds can
include a one-shot provider begin-upload instruction, preserving the complete
frozen upload duration. Default and `production-release` artifacts reject it;
production builds reject testing features at compile time. See the
[daily rules](docs/DAILY_LOTTERY_FIXED_RULES.md) and
[capability ABI](audit/schemas/daily-test-controls-v1.json).
The testing-controls CI builds each flavor twice with the pinned toolchain and
executes the resulting SBF. Its public fixture artifacts are qualification inputs,
not live deployments or release approval.

## Release status

This is a development candidate. It has not been deployed or activated. Existing program IDs in source are development inputs, not authorization to replace a deployed program. A network release needs fresh IDs, exact public source/build records, reviewed authority/configuration and a separate activation decision. Reproducibility and automated tests do not constitute a security audit.

Historical source commits remain in this repository's Git history. Consolidated source is promoted as new public commits; no private Git history is merged. Public commits and GitHub write actions use Plexer44. Operational credentials and keypairs do not belong here.

## V1 tasks and final brand review

The `brand-review` feature in `programs/giveaways-v1` adds immutable required
tasks, complete public per-task evidence and final brand approve/reject decisions.
`staging-afresh-v1` selects this schema for a fresh epoch. The original default and
devnet schemas retain their separate bindings; existing deployed programs remain
immutable. The contract version remains 1. The generated
[brand-review IDL](audit/schemas/giveaways-brand-review-v1.idl.json) names every
instruction and account; [audit tools](audit/README.md) cover independent hashes
and actual-runtime capture validation.

The brand seals 1–32 tasks before funding. Participants publish every required
response and seal their current revision before entry close, then reveal during
the finalized upload phase. The host commits a fixed pool and ranking. Each final
brand rejection consumes the next ranked candidate; the brand cannot select a
wallet or regenerate randomness. Approvals vest together. Exhaustion with some
approvals shares the net pool among them; zero approvals or unresolved expiry
refunds the pool without fees. Vested awards remain claimable. Public text and
a brand decision do not independently establish an external social action.

Bounds are 256 bytes per response, four retained versions per task, 8,192 current
evidence bytes, 4,096 admitted participants and 1,000 winner slots. Pool/wave
scans process four accounts at a time, ranking waves retain 32 keys, and seal
chunks process eight tasks. Repeated scans have quadratic worst-case total work.
Native development execution, public CI reproducibility, a security audit and a
qualified environment release are distinct claims. This source update deploys
and activates nothing. Fixed hash-domain suffixes retain encoding identifiers;
they do not define the product version.
