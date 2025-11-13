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
    RUST_LOG=info EIC_CONFIG_FILE=docker/eic-service/config.toml cargo run --package bcr-wdc-eic-service

run-ens-service:
    RUST_LOG=info ENS_CONFIG_FILE=docker/ens-service/config.toml cargo run --package bcr-wdc-ens-service

# to build docker containers
build-docker-base-image:
    docker build --ssh default -t wildcat-auxiliary/base-image -f docker/base-image/Dockerfile .

build-docker-eic-service: build-docker-base-image
    docker build -t wildcat/eic-service -f docker/eic-service/Dockerfile .

build-docker-ens-service: build-docker-base-image
    docker build -t wildcat/ens-service -f docker/ens-service/Dockerfile .

build-docker-images: build-docker-eic-service build-docker-ens-service
