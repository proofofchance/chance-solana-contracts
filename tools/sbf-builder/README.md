# Latest stable SBF builder candidate

This is a reproducible recipe candidate, not a published builder digest or release
qualification. It layers official Rust 1.100.0, official cargo-build-sbf 4.6.0 and
SHA-verified platform-tools 1.58 on the current official verifier image for Agave
4.3.0. The cached older `ff3...` image is not an input to this recipe.

`pins.json` identifies every upstream asset and immutable base. The tooling
`Cargo.lock` updates compatible dependencies of the unchanged upstream 4.6.0
source for the pinned host compiler; current RustSec reports no findings in that
lock. Upstream direct constraints remain, including keypair 4.0.0 and reqwest
0.12. Source archives, extracted tool trees, registry cache, context and binaries
must stay outside the checkout. No credentials or configuration from CARGO_HOME
are copied; only canonical locked crate archives and exact metadata are admitted.

Use a temporary asset directory containing the two files named in `pins.json`.
Download only their official listed URLs and verify their listed SHA-256 values.
Extract the verified cargo-build-sbf crate into a separate temporary directory,
replace only its Cargo.lock with this reviewed lock, and run `cargo +1.100.0 fetch
--locked --manifest-path <temporary-tool-source>/Cargo.toml` using an isolated
CARGO_HOME. Fetch each program's independent locked dependencies into that same
isolated cache. These fetches use canonical crates.io; they do not modify program
source or any existing release.

Prepare a new context outside the checkout:

```sh
python3 tools/sbf-builder/prepare.py \
  --assets <temporary-assets> --cargo-home <isolated-cargo-home> \
  --source "$PWD" --output <new-temporary-context>
docker build --platform linux/amd64 --network=none --pull=false \
  --build-arg SOURCE_DATE_EPOCH=0 --provenance=false \
  --tag chance-sbf-builder:4.6.0-platform1.58 <temporary-context>
```

Both exact official base images must already be present. Preparation verifies all
assets, lockfile checksums and required canonical registry records before writing
the context. It includes the three production program locks' dependencies so the
resulting builder can build offline without a verifier-side vendoring mutation.
The Dockerfile checks upstream asset hashes again. It normalizes copied input and
tool timestamps and sets the declared build epoch; actual repeatability still
requires independent builds. The final image contains no tool build target.

Record the actual local image ID from `docker image inspect`, its exact recipe and
lock hashes, versions and independent ELF byte comparisons. A real local image ID
can be passed to `solana-verify 0.5.2 build --base-image <actual-local-image-id>`.
It is not a published registry digest. Do not replace the currently admitted
builder pin until actual image/compiler qualification and private/public ELF
reproduction pass. A CI consumer must reconstruct and verify this recipe or use a
separately published and approved immutable image; never invent a remote digest.
No build command deploys a program, starts a ledger or changes authority.
