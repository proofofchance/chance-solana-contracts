FROM --platform=linux/amd64 rust@sha256:e9d95b74296f7126a83ae3d041aeaba1ff4c478f13abc5cc034fcf7c131c3d4a AS host_rust
FROM --platform=linux/amd64 solanafoundation/solana-verifiable-build@sha256:12fd4c0a0790f0fc41ef74b0cdb6bccc167ba6137eb1adb749bac649481c86bd AS current_base
COPY --from=host_rust /usr/local/rustup /usr/local/rustup
COPY --from=host_rust /usr/local/cargo /usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
ENV PATH=/usr/local/cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin
ARG SOURCE_DATE_EPOCH=0

FROM current_base AS cli_build
WORKDIR /opt/chance-sbf-tool
ENV CARGO_TARGET_DIR=/opt/chance-sbf-tool/target
COPY inputs/cargo-build-sbf-4.6.0.crate /tmp/tool.crate
RUN printf '%s  %s\n' 7699013848ea2373e7fc49846096d37c12b1efbfaffffb3724a2642d713fe08c /tmp/tool.crate | sha256sum -c - \
 && tar -xzf /tmp/tool.crate --strip-components=1 -C /opt/chance-sbf-tool
COPY Cargo.lock /opt/chance-sbf-tool/Cargo.lock
COPY cargo-home/registry /opt/offline-cargo/registry
RUN printf '%s  %s\n' ce3ace561e83806334a119d723a485a9ac2a7fa801979612b875ec19aa1ef505 /opt/chance-sbf-tool/Cargo.lock | sha256sum -c - \
 && CARGO_HOME=/opt/offline-cargo CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0 \
 cargo +1.100.0 build --release --locked --offline --bin cargo-build-sbf \
 && touch -d @0 /opt/chance-sbf-tool/target/release/cargo-build-sbf

FROM current_base AS platform_tools
COPY inputs/platform-tools-linux-x86_64-v1.58.tar.bz2 /tmp/platform-tools.tar.bz2
RUN printf '%s  %s\n' 7b82eb420b5aebd1e5590db0ae25c523eca06724d4238ae02139690535636c37 /tmp/platform-tools.tar.bz2 | sha256sum -c - \
 && mkdir -p /opt/chance-sbf/v1.58/platform-tools \
 && tar -xjf /tmp/platform-tools.tar.bz2 -C /opt/chance-sbf/v1.58/platform-tools \
 && find /opt/chance-sbf -exec touch -h -d @0 {} +

FROM current_base AS builder
COPY --from=cli_build /opt/chance-sbf-tool/target/release/cargo-build-sbf /usr/local/cargo/bin/cargo-build-sbf
COPY --from=platform_tools /opt/chance-sbf /opt/chance-sbf
COPY cargo-home/registry /usr/local/cargo/registry
ENV SOLANA_PLATFORM_TOOLS_DIR=/opt/chance-sbf CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0
WORKDIR /build
RUN rustc --version | grep '^rustc 1.100.0 ' \
 && cargo-build-sbf --version | grep '^cargo-build-sbf 4.6.0$' \
 && /opt/chance-sbf/v1.58/platform-tools/rust/bin/rustc --version | grep '^rustc 1.95.0-dev '
LABEL org.opencontainers.image.title="Chance Solana stable SBF builder candidate" \
 org.opencontainers.image.version="cargo-build-sbf4.6.0-platform1.58" \
 proofofchance.qualification="requires independent ELF reproduction"
