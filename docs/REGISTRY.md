# Solana registry and atomic creation

The native `programs/registry` executable shares byte-level Borsh schema 1 with both SDK generations through `crates/registry-wire`. Every address in the shared wire schema is 32 raw bytes. Registry PDAs retain release history, lifecycle reasons, canonical business keys and a sequential instance inventory independently of the owner program's operational accounts. Instances remain owned and settled by their original program.

## Immutable release gate

A registry namespace is `[registry, governance_pubkey]`. Initialization requires that governance signer and a nonzero activation delay. Governance may be a multisig-controlled signer. Its address and the guardian are fixed for this namespace. Changing governance requires separate future design; there is no hidden authority replacement instruction.

Registration records domain, series, full-source/build reference hash, executable digest, program ID, slot, sequence and activation time. Activation requires the configured delay, an immutable upgradeable-loader ProgramData account for both the target and the registry itself, and an exact target executable SHA-256 match. Hashing omits the loader's 45-byte metadata and strips trailing zero padding, matching solana-verify's binary-hash convention. The source reference is a commitment; build/source matching is established separately by reproducible verification, not proved by its existence in the registry.

Activation of large executables needs an explicit compute budget (tests use the 1,400,000 CU transaction ceiling). Measure the exact candidate executable before release. Unsupported loaders are rejected. No development flag bypasses immutability. Tests load immutable programs into isolated LiteSVM; no deployment authority has been revoked on a network.

Governance activates; governance or guardian pauses/discontinues. Resume waits another activation delay. Discontinuation is terminal. Only governance may set/correct a successor, and it must have a higher registration sequence, the same domain and the same series. Every lifecycle change creates a new permanent record with actor, slot, timestamp, reason and resulting status/successor. This ordering prevents cycles. Corrections append records instead of erasing prior reasons.

## Creation and namespaces

Each domain config is bound once, before any creation, to the registry executable and namespace. A binding validates registry ownership, schema, release domain, owner-program identity and PDA seeds. Creation includes a CPI signed by the owner program's `[chance-release, registry_config]` PDA. The registry checks the release is active and the newly allocated instance is owned by that exact program. Omitting or spoofing the CPI accounts fails the entire creation transaction, including funding and local sequence changes.

The registry creates two permanent accounts atomically:

- `[business, registry_config, domain_byte, business_key]` points to the canonical inventory entry and enforces uniqueness.
- `[instance, registry_config, sequence_u64_le]` records owner program, instance, release, creator, fixed creation-state hash, business key and creation slot. Inventory starts at 1.

Daily business keys hash `chance-daily-key-v1 || series[32] || UTC_day_u64_le`. Daily creates use the frozen buy-start day. The new native instruction tag 19, `CreateScheduledLottery { buy_start_unix: i64 }`, supports future starts and rejects backdating. Legacy tag 2 starts immediately but has the same mandatory registry gate. Tag 18 binds the registry.

Giveaway business keys hash `chance-giveaway-key-v1 || creator[32] || creator_nonce_u64_le`. `create_giveaway_with_nonce` exposes a durable client nonce across releases. The legacy create method uses its local giveaway ID as the nonce and can encounter legitimate cross-release collisions; new clients must use the explicit nonce method. Sequential local IDs still determine the giveaway PDA. Active starts cannot be backdated.

A lamport donation cannot squat a canonical registry PDA: creation allocates and assigns an empty system-owned PDA with its signing seeds, topping up rent as needed. Existing registry-owned records always reject overwriting. Historical settlement instructions do not consult release status.

## Account ordering

Registry initialize: config (writable), governance (signer/payer), system.

Register: config (writable), release (writable), governance (signer/payer), system, target executable. Release PDA is `[release, registry_config, target_program]`.

Lifecycle: config (writable), release (writable), actor (signer/payer), system, new record (writable). Record PDA is `[record, registry_config, next_record_sequence_u64_le]`. Activation additionally takes target executable, target ProgramData, registry executable, registry ProgramData. Successor assignment additionally takes the successor release.

RecordInstance CPI: config (writable), release, owner-program PDA signer, instance, creator (signer), rent payer (signer/writable), system, new inventory entry (writable), business key (writable). The runtime signer check prevents direct transactions impersonating a program PDA.

Daily creation appends six accounts after the existing five: registry executable, registry config (writable), release, release signer, entry (writable), business key (writable). Giveaway creation adds the same six named accounts. BindRegistry takes domain config (writable), domain authority (signer), registry executable, registry config, release.

The creation-state hash is SHA-256 over the full Borsh instance data at creation, excluding account discriminator. Later lifecycle fields change, so recomputing this hash over today's whole account is incorrect. Auditors reconstruct the original state from creation transaction inputs, fixed fields and versioned layouts.

## Deployment and validation limits

These program sources still carry imported development identifiers where applicable. Do not deploy over an existing program. The release manifest must assign fresh program IDs, initialize and bind the intended authority/config before freezing, verify both source and binary, and only then consider irreversible immutability/activation under a separate environment action. Network identity is the genesis hash plus registry/program/account addresses; the chain-local registry does not by itself prove a claimed network name.

Runtime tests cover terminal lifecycle, delay/guardian boundaries, mutable ProgramData rejection, real daily-program cross-release collisions and rollback, PDA prefunding, direct creation without the registry accounts, independent giveaway creator nonce enforcement, and provider-timeout refund after discontinuation. Runtime coverage also includes interrupted giveaway selection, vested payouts after retirement without the provider signature, and weighted daily draws. The public verifier and consumer layouts are documented in audit/README.md; archival network acceptance and public promotion remain distinct gates. Automated tests are not a comprehensive security audit.
