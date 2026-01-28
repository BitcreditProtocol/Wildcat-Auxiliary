ARG WILDCAT_BASE_IMAGE=wildcat-auxiliary/base-image
FROM ${WILDCAT_BASE_IMAGE} AS rust-builder

##############################
## Create image
##############################
FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install --quiet --no-install-recommends --no-install-suggests --yes openssl curl ca-certificates \
  && apt-get clean \
  && rm --recursive --force /var/lib/apt/lists/*

WORKDIR /eic-service
COPY --from=rust-builder /wildcat/target/release/bcr-wdc-eic-service .

# Expose web server port
EXPOSE 3338

ENTRYPOINT ["/eic-service/bcr-wdc-eic-service"]
