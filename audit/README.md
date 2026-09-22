# Public Solana verification tools

The verifier uses public source-derived account schemas, a separately reproduced ELF, and finalized Solana RPC evidence. It never needs private Git history. Python 3.10+ is sufficient; there are no Python package dependencies. Set `CHANCE_SOLANA_RPC_URL` without placing credentials in command arguments or reports.

```
python3 audit/inspect_instance.py --genesis-hash <network-genesis> --registry <program-id> --inventory <registry-instance-PDA>
python3 audit/replay_instance.py --genesis-hash <network-genesis> --registry <program-id> --inventory <registry-instance-PDA>
```

An inventory PDA uses `[instance, registry-config, sequence-u64-le]`. Registry records are permanent even when operational participant/vault accounts close. The schema and account derivations are documented in `docs/REGISTRY.md`. The caller selects the trusted registry and genesis; an organization name or program label is not a network identity.

Inspection decodes related accounts in one finalized `getMultipleAccounts` bank snapshot. It checks ownership, PDA seeds, release and creator binding, account versions, immutable upgradeable-loader metadata, deployment slots and the executable digest recorded in the release. A retired release can still own a valid historical instance. The supplied RPC is trusted for authentic/complete history; this is not a light-client consensus proof.

## Source and executable match

Reproduce the exact public commit and each selected library using `docs/REPRODUCIBLE_BUILDS.md`. Then create a per-program build record:

```
python3 tools/build_manifest.py --repository https://github.com/proofofchance/chance-solana-contracts --commit <full-checked-out-public-SHA> --library giveaways --elf target/deploy/giveaways.so --output /tmp/giveaways-build.json
```

The tool records the source URL/commit, selected manifest/library, mount root, lock hash, features, pinned image and verifier. It does not perform or certify a build. The registry's `source_hash` must commit to the canonical `source` JSON object (sorted keys, compact separators, UTF-8); `sourceReferenceSha256` contains that digest. The executable digest strips trailing zero padding, matching registry activation and solana-verify. A deployment manifest must separately record actual genesis, registry, program, ProgramData, slot and authority state.

Pass `--build-manifest /tmp/giveaways-build.json --elf target/deploy/giveaways.so` to either verifier command. `buildMatch: match` means the supplied reproduced binary, public source-reference commitment and deployed executable agree. It does not authenticate who ran the build, substitute for fetching/building that public commit, or prove security. A missing build record stays `unavailable`.

Keep historical source-reference objects unchanged after a repository rename: their committed URL may use the old redirect. Do not silently rewrite a hashed manifest to the new name. Record new releases with the canonical name. Imported development IDs are not a deployment manifest and must not be used to overwrite existing programs.

## Execution evidence

Replay enumerates signatures, retrieves their containing finalized blocks for transaction order and verifies successful execution. JSON log text is attributed using the runtime invocation stack; logs from other programs and failed CPI subtrees cannot spoof an event. Truncated traces and missing history fail closed.

Participation, attestation, exclusions and full reveal bytes are reconstructed from transactions and program-authenticated logs. Provider receipt checks require the preceding executed Ed25519 verification instruction to contain the frozen signer, commitment, instance and participant message. Direct reveal remains a participant-signed path. Batch disclosure and provider finalization are checked separately.

Lottery draws use an independent cumulative-weight calculation. Giveaway ranking uses a full sort and threshold calculation instead of the on-chain radix passes. Their inputs include the ordered eligible population and reveal aggregate. Payouts are checked against actual recipient/vault balance changes, accounting for the transaction fee. Creation-state hashes are reconstructed from the original zeroed lifecycle fields and frozen parameters; hashing today's entire mutable instance account would be wrong.

`eventReplay` is separate from `buildMatch` and `securityReview`. Event completion and financial closure have separate fields. Unpaid principal stays an obligation after selection, and no report treats a source hash or Merkle membership alone as an exhaustive audit. The verifier currently supports a single top-level instance instruction per transaction. Bundled/CPI instance calls, arbitrary extra transfers and other unsupported historical evidence must be reported explicitly, never silently skipped. Participant snapshot reconciliation and broader archival/provider compatibility remain acceptance work.

Exit codes: replay `0` for its bounded observed-execution checks, `1` for contradictory evidence and `2` for incomplete/unsupported evidence. Inspection returns `2` even when ownership/build checks pass because event replay and security review are separate. Reports never claim absence of vulnerabilities or resistance to selective disclosure, biased participation or provider influence.

## Schemas and tests

`tools/export_schemas.py` exports explicitly supported Borsh account layouts and source-file hashes. Unknown field types reject generation. It is not an Anchor instruction IDL. `tools/idl-export` uses the pinned Anchor IDL builder for the giveaway program; `cargo run --locked --manifest-path tools/idl-export/Cargo.toml` produces the consumer IDL. The native daily program's instruction enum/account ordering remains its public ABI source.

The runtime suite can export real SBF account bytes, successful transaction logs and pre/post balances into isolated test fixtures:

```
CHANCE_AUDIT_FIXTURE=/tmp/giveaway-audit.json CHANCE_DAILY_AUDIT_FIXTURE=/tmp/daily-audit.json cargo test --locked --manifest-path programs/registry/Cargo.toml
CHANCE_AUDIT_FIXTURE=/tmp/giveaway-audit.json CHANCE_DAILY_AUDIT_FIXTURE=/tmp/daily-audit.json python3 -m unittest discover -s audit/tests -v
```

These fixtures represent an isolated LiteSVM bank; its RPC transport/block identity is test data, not a deployed network. CI creates them from the exact reproducibly built binaries and exercises both positive replay and tampered/missing evidence. No test fixtures populate product surfaces. Network archival verification and public publication remain distinct gates.

Unrecognized instance events return incomplete; frozen-window mutations fail. Selection and payout checks are reported only when those actions occurred. Participant account cleanup is currently unsupported rather than silently omitted.

Completeness checks compare successful instance transactions in every fetched canonical block against RPC signature pagination. A missing signature in a fetched block is incomplete evidence. This does not establish completeness for a whole block omitted by the RPC. Daily payout reconciliation compares exact winner bitmap positions, not merely the number of set bits.
