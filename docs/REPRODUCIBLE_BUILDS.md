# Reproducing the consolidated source

The registry, daily lottery and giveaways programs each have an independent manifest and dependency lock: production program APIs use Solana 5.2.0, and giveaways uses Anchor 1.2.2 with its `solana-v4` compatibility feature. The repository is not a single Cargo workspace. Its root `Cargo.lock` is a symlink to the daily program lock solely because solana-verify 0.5.2 requires a lock at the mount root to discover a Solana SDK version. The explicit image digest below overrides SDK-based image selection. Each actual `--manifest-path` build consumes that program's own lockfile; the root symlink does not resolve giveaways dependencies.

Mount the entire repository so shared crates and build-policy sources remain available. `.cargo/config.toml` fixes the output location at repository `target/`, which is where solana-verify looks for the selected library. Run from a clean clone at a full commit SHA, with Docker and solana-verify 0.5.2 installed:

```sh
export DOCKER_DEFAULT_PLATFORM=linux/amd64
solana-verify build --library-name daily_lottery --base-image solanafoundation/solana-verifiable-build@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd "$PWD" -- -Znext-lockfile-bump
solana-verify build --library-name giveaways --base-image solanafoundation/solana-verifiable-build@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd "$PWD" -- -Znext-lockfile-bump
```

The CI explicitly checks out the PR head SHA (not an ephemeral merge commit) and includes `chance_registry` as a third library and runs the registry/daily LiteSVM suites against the resulting production binaries. It independently builds two clean trees of the candidate commit per library and compares exact ELF bytes. It records the commit, image, binary and SHA-256 as build evidence. A successful reproducibility check establishes repeatable build inputs only. It does not establish an on-chain executable match, correct authority state, or security. These commands never deploy or revoke authority.

Before public promotion, inspect these job results, promote only the allowlisted source/build inputs, then repeat from the public candidate commit. On-chain verification must separately record network, program ID, ProgramData address, executable hash, deployment slot and upgrade authority. The programs may share one public repository but retain separate verification records, selected library names and release identities. No remote verification registration or repository rename has occurred yet.

## Consumer artifacts

Account schemas live in `audit/schemas/accounts-v1.json`; the giveaway instruction IDL is `audit/schemas/giveaways-v1.idl.json`. Regenerate with `python3 tools/export_schemas.py` and `cargo run --locked --manifest-path tools/idl-export/Cargo.toml`. The exporter uses pinned `anchor-lang-idl` 0.2.0 and the program’s locked Anchor 1.2.2 macros, with safety linting enabled. It clears only the child toolchain environment override to avoid the builder’s literal `{toolchain}` argument bug; `rust-toolchain.toml` still selects the compiler. The root account schema check and runtime replay run against the exact reproducible artifacts.

Use `tools/build_manifest.py` only after independently building the selected public commit. It records source/build inputs and executable digest without claiming deployment. The registry source-reference hash commits to canonical JSON, so retain old source references unchanged after a repository rename. See `audit/README.md`.

The pinned `solana-verify` already supplies `--locked` to Cargo. Do not repeat it in forwarded arguments: Cargo rejects duplicate `--locked`. Exact-head checkout and clean-tree comparison remain required.

## Fresh giveaway Devnet release

The separate `Giveaway Devnet reproducible build` workflow forwards
`--features devnet-v1` and compares two clean builds. Its artifact is
`verified-giveaways-devnet-v1`; default runtime tests never merge it over the
original `giveaways.so`. Record manifests with `--library giveaways --features
devnet-v1`, and export the feature IDL with the IDL exporter argument
`--devnet-v1`. It writes `audit/schemas/giveaways-devnet-v1.idl.json` independently
of the default IDL. See [release selection](GIVEAWAY_RELEASE_SELECTION.md) for the
public program/bootstrap addresses and permissionless creation rules.

## Current runtime and builder boundary

After production builds, run `SBF_OUT_DIR="$PWD/target/deploy" cargo test --locked
--manifest-path tests/runtime/Cargo.toml`. Use `--no-default-features --features
daily-suite` or `registry-suite` for one domain. The independent harness isolates
LiteSVM's current API constraint and loads actual production SDK 5.2 ELF files.
The shared-source daily model is host-only and cannot substitute for an ELF.

The source Rust pin is the host toolchain. Verifier 0.5.2 explicitly uses the
container's active toolchain and image-owned SBF platform-tools. Native 1.58
validation and a prepared builder context do not establish official-image or
private/public Docker reproduction. See the [candidate recipe](../tools/sbf-builder/README.md)
and [current dependency status](../README.md#current-dependency-candidate).

## Reviewed test-only advisory baseline

The scheduled dependency job builds the [reviewed scanner](../tools/dependency-audit/README.md)
and audits every independent public lock. Production and tooling retain strict
gates. The current production locks have no reported vulnerabilities, with the
upstream bincode maintenance warning retained.

On 2026-11-17 the workspace user approved only the exact isolated LiteSVM runtime
[baseline](../audit/runtime-advisory-baseline.json): three vulnerabilities and six
informational findings, the exact lock and 62 reviewed source/feature hashes. The
raw scanner still exits 1 and its complete JSON, stderr and status are printed and
retained even when the gate fails. New or changed findings, versions, advisory
payloads, categories or source context require a new review. Ignore flags,
severity/target filters and use of this approval for another lock are refused.

Required Agave precompiles still pull ed25519-dalek 1.0.1
([RUSTSEC-2022-0093](https://rustsec.org/advisories/RUSTSEC-2022-0093.html)),
curve25519-dalek 3.2.0
([RUSTSEC-2024-0344](https://rustsec.org/advisories/RUSTSEC-2024-0344.html)) and
crypto-bigint 0.5.5
([RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html)).
The reviewed [Ed25519 precompile](https://github.com/anza-xyz/agave/blob/v4.3.2/precompiles/src/ed25519.rs)
verifies public key/signature/message inputs; it does not call old signing APIs.
Fixtures sign with keypair 3.1.2/dalek 2.2.0. The old k256 construction/signing
helper is not called; the [secp256k1 precompile](https://github.com/anza-xyz/agave/blob/v4.3.2/precompiles/src/secp256k1.rs)
performs public-signature recovery. Rand 0.7.3's logger/RNG conditions were not
found: its log feature is disabled and the harness has no RNG-calling custom
logger. The five maintenance warnings and rand unsoundness remain visible. No
production signing keys enter this harness. These are reviewed reachability
observations, not patched-library claims. Remediation requires a compatible
patched upstream simulator/precompile release or a separately reviewed maintained
integration with consensus-equivalence evidence; do not drop precompile checks.

## Recorded native migration checks

The final source graph compiled registry, daily, legacy giveaway and brand-review
giveaway ELF files with official cargo-build-sbf 4.6.0/platform-tools 1.58.
The actual production ELF runtime checks passed: 29 daily, eight registry and 11
brand-review tests. The brand fixture retained the default compute and 1,232-byte
packet gates, with 13,875 successful calls, a maximum 92,345 CU and 874 bytes.
All 52 audit/replay/tamper tests passed with real execution fixtures. Current
IDL exports matched the existing source-bound schemas byte for byte.

Two-participant finalization measured 217,777–246,109 CU and four-reveal uploads
223,218–239,728 CU. Their fixture helpers use the same explicit bounded 1,400,000-CU
request already used by operator transaction builders; other transactions keep
default limits. These fixture measurements do not prove arbitrary batch limits
or live-chain qualification. Native validation is separate from the pending
two-clean-tree Docker reproduction and any network release admission.
