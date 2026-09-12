# syntax=docker/dockerfile:1
FROM rust:1.88-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY openapi ./openapi
RUN cargo build --release -p mq-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 \
  && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/mq-server /usr/local/bin/mq-server
# Bind via PORT (Railway) or MQ_BIND; do not hardcode 8088 here.
EXPOSE 8088
CMD ["mq-server", "serve"]
