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
