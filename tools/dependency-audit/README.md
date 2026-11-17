# Reviewed cargo-audit 0.23.0 build

This self-contained source recipe builds the current stable scanner with Rust
1.100.0 and a reviewed lockfile. Copy `install.py`, `pins.json`, `Cargo.lock`,
`test_install.py` and this README together. It needs Python 3.11 or newer and an
already available Rustup Cargo shim with the 1.100.0 toolchain. It changes no global
Cargo installation or default toolchain.

The official 0.23.0 crate is pinned to SHA-256
`956f85aedfb64db887ec38466f982fc2b1ae55de726d4a12f2fddd5b812c6446`.
Its shipped lock includes active Rustix 1.1.5 (`RUSTSEC-2026-0486`) and optional
Quinn entries affected by `RUSTSEC-2026-0523`, `RUSTSEC-2026-0524` and
`RUSTSEC-2026-0538`. The reviewed overlay changes exactly three package versions
and their official archive checksums:

| Package | Shipped | Reviewed |
| --- | --- | --- |
| rustix | 1.1.5 | 1.1.6 |
| quinn-proto | 0.11.18 | 0.11.19 |
| quinn-udp | 0.5.15 | 0.5.16 |

Quinn UDP remains on its upstream-required 0.5 line. Other package selections,
including Windows dependency edges, remain unchanged. Both the shipped and
reviewed lock hashes are in `pins.json`. Cargo verifies dependency archives
against the reviewed lock; the recipe never resolves a fresh unlocked graph.

## CI installation

Use new, distinct work and installation directories. The normal path downloads
only the exact official source archive, builds with `--release --locked`, and
self-audits the complete scanner lock against freshly fetched public RustSec data
in its isolated Cargo home. Any vulnerability, informational warning, ignored
advisory or failed self-audit prevents installation.

```sh
python3 -B tools/dependency-audit/install.py \
  --work-dir "$RUNNER_TEMP/chance-cargo-audit-build" \
  --install-root "$RUNNER_TEMP/chance-cargo-audit-installed"
echo "$RUNNER_TEMP/chance-cargo-audit-installed/bin" >> "$GITHUB_PATH"
```

Use the installed executable for repository scans, retaining each repository's
explicit advisory policy. The scanner self-audit does not ignore repository
findings or turn simulator-only findings into production-artifact evidence.

The installer writes `qualification.json` beneath the installation root. It binds
the source, both locks, installer, pins, platform-specific binary and advisory
snapshot hashes. Build and raw self-audit logs stay in the work directory. Host
binary hashes are evidence for that build, not a claim of reproducibility across
operating systems. The installer rejects unsafe archive paths, duplicate entries,
links, special files and oversized sources before extraction. It preserves
existing destinations and stops only its own build process group if free disk
space falls below 4 GiB. It uses at most two build jobs and a 20-minute build limit.

## Offline qualification

Previously downloaded public inputs can be reused explicitly. The archive still
requires the official checksum, and Cargo still requires the reviewed lock. An
explicit cache or target directory must be isolated; neither may use the global
Cargo installation. No caches, secrets or configuration are copied automatically.

```sh
python3 -B tools/dependency-audit/install.py \
  --work-dir /absolute/new-build \
  --install-root /absolute/new-installation \
  --archive /absolute/cargo-audit-0.23.0.crate \
  --cargo-home /absolute/isolated-cargo-cache \
  --target-dir /absolute/isolated-build-target \
  --advisory-db /absolute/inspected-advisory-db \
  --offline
```

The offline path cannot fetch missing inputs and records the supplied advisory
snapshot fingerprint. Without an explicit snapshot, the normal CI path refreshes
RustSec. A new advisory can therefore deny a later installation; update the
reviewed lock and qualification rather than disabling that check.

Run the focused installer security tests with:

```sh
python3 -B -m unittest discover -s tools/dependency-audit -p test_install.py -v
```

The recipe neither changes an application's dependency graph nor authorizes a
commit, publication, deployment, host change or chain operation.
