##############################
## Build Rust backend
##############################
FROM rust:1.91-bookworm AS rust-builder
WORKDIR /wildcat

RUN apt-get update && apt-get install --quiet --no-install-recommends --no-install-suggests --yes \
    protobuf-compiler git

ENV SQLX_OFFLINE=true


COPY . .

RUN cargo build --release
