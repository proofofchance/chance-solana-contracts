# Reproducing the consolidated source

The registry, daily lottery and giveaways programs each have an independent manifest and dependency lock: daily lottery uses Solana 4, while giveaways uses Anchor 0.32.1 and Solana 2.3. The repository is not a single Cargo workspace. Its root `Cargo.lock` is a symlink to the daily program lock solely because solana-verify 0.4.11 requires a lock at the mount root to discover a Solana SDK version. The explicit image digest below overrides SDK-based image selection. Each actual `--manifest-path` build consumes that program's own lockfile; the root symlink does not resolve giveaways dependencies.

Mount the entire repository so shared vendored crates remain available. `.cargo/config.toml` fixes the output location at repository `target/`, which is where solana-verify looks for the selected library. Run from a clean clone at a full commit SHA, with Docker and solana-verify 0.4.11 installed:

```sh
export DOCKER_DEFAULT_PLATFORM=linux/amd64
solana-verify build --library-name daily_lottery --base-image solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8 "$PWD" -- -Znext-lockfile-bump
solana-verify build --library-name giveaways --base-image solanafoundation/solana-verifiable-build@sha256:ff3b148fb6adc3025c46ac38f132f473ccbdc4391f253234d98aa6519aec07f8 "$PWD" -- -Znext-lockfile-bump
```

The CI explicitly checks out the PR head SHA (not an ephemeral merge commit) and includes `chance_registry` as a third library and runs the registry/daily LiteSVM suites against the resulting production binaries. It independently builds two clean trees of the candidate commit per library and compares exact ELF bytes. It records the commit, image, binary and SHA-256 as build evidence. A successful reproducibility check establishes repeatable build inputs only. It does not establish an on-chain executable match, correct authority state, or security. These commands never deploy or revoke authority.

Before public promotion, inspect these job results, promote only the allowlisted source/build inputs, then repeat from the public candidate commit. On-chain verification must separately record network, program ID, ProgramData address, executable hash, deployment slot and upgrade authority. The programs may share one public repository but retain separate verification records, selected library names and release identities. No remote verification registration or repository rename has occurred yet.

## Consumer artifacts

Account schemas live in `audit/schemas/accounts-v1.json`; the giveaway instruction IDL is `audit/schemas/giveaways-v1.idl.json`. Regenerate with `python3 tools/export_schemas.py` and `cargo run --locked --manifest-path tools/idl-export/Cargo.toml`. The exporter uses pinned `anchor-lang-idl` 0.1.3 and the program’s locked Anchor 0.32.1 macros, with safety linting enabled. It clears only the child toolchain environment override to avoid the builder’s literal `{toolchain}` argument bug; `rust-toolchain.toml` still selects the compiler. The root account schema check and runtime replay run against the exact reproducible artifacts.

Use `tools/build_manifest.py` only after independently building the selected public commit. It records source/build inputs and executable digest without claiming deployment. The registry source-reference hash commits to canonical JSON, so retain old source references unchanged after a repository rename. See `audit/README.md`.

The pinned `solana-verify` already supplies `--locked` to Cargo. Do not repeat it in forwarded arguments: Cargo rejects duplicate `--locked`. Exact-head checkout and clean-tree comparison remain required.

Public CI includes a per-program source/ELF manifest with each build artifact. See [historical source continuity](HISTORICAL_SOURCE.md) for preserved commits and the limits of the rename check.
