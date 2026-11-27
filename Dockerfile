FROM rust:1.95.0-trixie AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml README.md ./
COPY src/ src/
RUN cargo build --locked --release --bin evanescent

ARG TARGETARCH
# Checksums from https://github.com/helm/helm/releases/tag/v4.3.2.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) checksum=d1c0278d46beab79582d7e50f5b3dc206944ba8e577cde618513fd07daa53983 ;; \
      arm64) checksum=df31cb61680ef728f82c770e5280dd565617bee6b039d37e85c677288be4bbb0 ;; \
      *) echo "Unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl --fail --silent --show-error --location --retry 3 \
      "https://get.helm.sh/helm-v4.3.2-linux-${TARGETARCH}.tar.gz" -o /tmp/helm.tar.gz; \
    echo "$checksum  /tmp/helm.tar.gz" | sha256sum --check; \
    tar -xzf /tmp/helm.tar.gz -C /tmp "linux-${TARGETARCH}/helm"; \
    install -m 0755 "/tmp/linux-${TARGETARCH}/helm" /usr/local/bin/helm

FROM debian:trixie-slim

COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /build/target/release/evanescent /usr/local/bin/evanescent
COPY --from=build /usr/local/bin/helm /usr/local/bin/helm
COPY charts/api-postgres/ /opt/evanescent/charts/api-postgres/

ENV HOME=/home/evanescent
RUN install -d -o 65532 -g 65532 -m 0755 "$HOME"
WORKDIR /home/evanescent
USER 65532:65532

ENTRYPOINT ["/usr/local/bin/evanescent"]
