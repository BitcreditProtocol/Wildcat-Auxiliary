# print available targets
[group("project-agnostic")]
default:
    @just --list --justfile {{justfile()}}

# evaluate and print all just variables
[group("project-agnostic")]
evaluate:
    @just --evaluate

# print system information such as OS and architecture
[group("project-agnostic")]
system-info:
  @echo "architecture: {{arch()}}"
  @echo "os: {{os()}}"
  @echo "os family: {{os_family()}}"

# to run the services locally individually
run-eic-service:
    RUST_LOG=debug EIC_CONFIG_FILE=docker/eic-service/config.toml cargo run --package bcr-wdc-eic-service

run-ens-service:
    RUST_LOG=debug ENS_CONFIG_FILE=docker/ens-service/config.toml cargo run --package bcr-wdc-ens-service

run-ebill-service:
    EBILL_MNEMONIC="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" RUST_LOG=debug EBILL_CONFIG_FILE=docker/ebill-service/config.toml cargo run --package bcr-wdc-ebill-service

run-relay:
    RUST_LOG=debug cargo run --package bcr-wdc-relay

# to build docker containers
build-docker-base-image:
    docker build --ssh default -t wildcat-auxiliary/base-image -f docker/base-image/Dockerfile .

build-docker-eic-service: build-docker-base-image
    docker build -t wildcat/eic-service -f docker/eic-service/Dockerfile .

build-docker-ens-service: build-docker-base-image
    docker build -t wildcat/ens-service -f docker/ens-service/Dockerfile .

build-docker-ebill-service: build-docker-base-image
    docker build -t wildcat/ebill-service -f docker/ebill-service/Dockerfile .

build-docker-relay: build-docker-base-image
    docker build -t wildcat/relay -f docker/relay/Dockerfile .

build-docker-images: build-docker-eic-service build-docker-ens-service build-docker-ebill-service build-docker-relay

