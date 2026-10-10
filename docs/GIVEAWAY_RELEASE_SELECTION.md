# Permissionless giveaways and release selection

Reviewed 2026-10-10. A Solana giveaway factory is a versioned program plus its
configuration account. Calling that program creates a giveaway PDA and funded
vault; it does not deploy a new executable for each campaign. EVM instead uses a
fixed-target factory and a separately owned instance contract.

Any funded creator may create a giveaway through an active release. The provider
does not approve creators or sign creation. Provider-only reveal/finalization
authority, fixed at creation, is a separate role. Registry approval governs which
programs belong to the canonical Chance release inventory.

Release replacement means a fresh program/factory address, source commitment,
executable digest and interface digest. Do not upgrade an activated Solana program
in place. The registry already requires immutable target and registry executables
before activation. Daily lottery uses this same release replacement mechanism;
its provider-only creation and one-event-per-UTC-day business key differ from
permissionless giveaway creation.

The future version dropdown selects from an explicitly curated release catalog
for the exact chain/genesis and registry. Default to the highest approved release
sequence that is both visible and currently active. A displayed semantic version
must be pinned to that release; registration sequence alone is not a semantic
version. Revalidate its lifecycle and reviewed creation hash before signing.
Removing an older version from the creation catalog does not retire it on chain.
Pause/discontinue in the registry to block new direct creation when required.
Neither action changes already created giveaways or their original indexers,
settlement adapters, provider, fees and deadlines.

Campaign name, brand description, artwork and participation instructions belong
in versioned application metadata keyed to the real chain instance and creator.
They are not implemented as on-chain fields by this release. The creator's form
will separately approve payout, winners and immutable timing/fee/provider terms.
No frontend implementation or populated product records are claimed here.

## Fresh Devnet V1 candidate

The explicit `devnet-v1` feature embeds program
`FQxfsBE7fXuBbcxERsgRwi1AHsbSMdU7nWwhGz8ZRLUY` and initial configuration signer
`HMroJo6qBsFiodxFJeqU8VDBLif5P5kDFWmEFpibaKCQ`. Both are public addresses,
not private keys. The initializer checks that signer before accepting a config,
preventing another wallet from taking the provider role after immutable deployment.
Creation continues to require only the creator's signature. Each successor build
must receive its own explicit identity; this feature never patches an old ELF or
reuses historical deployment authorities.

The default build retains its original local program ID and initialization behavior
for isolated compatibility tests. It is not this live release. Feature-specific
ELF, IDL and source manifests are separate from default artifacts. Reproducibility
and host tests do not qualify a network deployment: registration, finalized account
binding, real creator transactions, persistent indexing and restart recovery remain
separate acceptance gates. This document records a candidate, not completion of
those gates.
