##############################
## Build Rust backend
##############################
FROM rust:1.91-bookworm AS rust-builder
WORKDIR /wildcat

RUN apt-get update && apt-get install --quiet --no-install-recommends --no-install-suggests --yes \
    protobuf-compiler git

RUN cargo install cargo-chef --locked --version 0.1.78

ENV SQLX_OFFLINE=true

##############################
## Dependency recipe
##############################
FROM rust-builder AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

##############################
## Build dependencies, then workspace
##############################
FROM rust-builder
COPY --from=planner /wildcat/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .

RUN cargo build --release
