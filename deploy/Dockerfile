# syntax=docker/dockerfile:1
FROM node:22-bookworm AS web
WORKDIR /src
COPY spikes/runtime/package*.json spikes/runtime/
RUN cd spikes/runtime && npm ci --no-audit --no-fund
COPY dashboard dashboard
COPY engine/shared engine/shared
COPY assets assets
COPY deploy/package-web.py deploy/package-web.py
RUN bash dashboard/build-web.sh && python3 deploy/package-web.py /web

FROM rust:1.96-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends libclang-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY engine engine
COPY dashboard/shared dashboard/shared
COPY dashboard/examples dashboard/examples
COPY deploy/login.html deploy/login.html
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/engine/target \
    cargo build --manifest-path engine/Cargo.toml --locked --release --bins && \
    mkdir /out && cp engine/target/release/talia-engine engine/target/release/talia-agent engine/target/release/talia-mcp engine/target/release/talia-bootstrap /out/

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl sqlite3 && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
COPY --from=web /web /opt/talia/web
USER 65532:65532
ENV TALIA_LISTEN=0.0.0.0 TALIA_WEB_ROOT=/opt/talia/web
EXPOSE 8080
ENTRYPOINT ["talia-engine"]
CMD ["/data/talia.sqlite3", "8080"]
