# Multi-stage Dockerfile for the standalone oryxis-relay server.
#
# Build context expects to be the repository root:
#   docker build -f crates/oryxis-relay/Dockerfile -t oryxis-relay .
#
# Stage 1: build the static binary inside a Rust toolchain image.
# We target musl so the final image can be `scratch` (no glibc, no
# libssl, no shell). aws-lc-sys is not pulled in here (no rustls);
# axum + tokio compile clean against musl out of the box.

FROM rust:1.83-slim-bookworm AS builder

# musl-tools for musl target; pkg-config + cc for any C deps that
# do show up transitively. Tiny image cost.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        musl-tools pkg-config build-essential \
    && rm -rf /var/lib/apt/lists/*

ARG TARGET=x86_64-unknown-linux-musl
RUN rustup target add ${TARGET}

WORKDIR /src

# Copy just the manifests first so Docker layer caching kicks in
# whenever sources change but dependency graph doesn't. The full
# tree is copied second.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --target ${TARGET} -p oryxis-relay \
    && cp /src/target/${TARGET}/release/oryxis-relay /oryxis-relay

# Stage 2: minimal runtime. We use distroless rather than scratch so
# the binary has /etc/ssl/certs available if we ever need outbound
# TLS (we don't today, but it costs ~2 MB and makes ops happy).
#
# distroless:static-debian12 is glibc-free; pairs with the
# musl-built binary cleanly.

FROM gcr.io/distroless/static-debian12:nonroot

LABEL org.opencontainers.image.title="oryxis-relay"
LABEL org.opencontainers.image.description="Self-hostable signaling + relay server for Oryxis sync"
LABEL org.opencontainers.image.source="https://github.com/wilsonglasser/oryxis"
LABEL org.opencontainers.image.licenses="AGPL-3.0-or-later"

COPY --from=builder /oryxis-relay /usr/local/bin/oryxis-relay

# Default port; override with --port / ORYXIS_RELAY_PORT.
EXPOSE 8080

# ORYXIS_RELAY_TOKEN must be supplied at runtime (the binary refuses
# to start without one).
ENTRYPOINT ["/usr/local/bin/oryxis-relay"]
