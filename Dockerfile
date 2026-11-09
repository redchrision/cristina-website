# syntax=docker/dockerfile:1.7
# Multi-stage build: frontend (Node) + backend (Rust) -> ~30 MB distroless image.

FROM node:22.20-bookworm-slim AS web
WORKDIR /src
COPY content ./content
COPY web/package.json web/package-lock.json ./web/
RUN --mount=type=cache,target=/root/.npm cd web && npm ci
COPY web ./web
ARG VITE_ANALYTICS=off
RUN cd web && VITE_ANALYTICS=$VITE_ANALYTICS npm run build

FROM rust:1.96-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY server ./server
COPY loadgen ./loadgen
COPY content ./content
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p site-server \
 && cp target/release/site-server /site-server \
 && mkdir /data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=server /site-server /usr/local/bin/site-server
COPY --from=web /src/web/dist /app/web/dist
COPY --from=server --chown=65532:65532 /data /data
WORKDIR /app
ENV BIND=0.0.0.0:8080 \
    METRICS_BIND=0.0.0.0:9090 \
    STATIC_DIR=/app/web/dist \
    DATABASE_URL=sqlite:///data/site.db?mode=rwc \
    LOG_FORMAT=json
VOLUME ["/data"]
EXPOSE 8080 9090
HEALTHCHECK --interval=15s --timeout=3s --retries=3 CMD ["site-server", "healthcheck"]
ENTRYPOINT ["site-server"]
CMD ["serve"]
