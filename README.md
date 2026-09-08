# Wildcat-Auxiliary

Wildcat auxiliary services

### Crates

The project consists of the following crates:

* `bcr-wdc-ebill-service` - E-Bill Service
* `bcr-wdc-eic-service` - E-Bill Identity Confirmation Service
* `bcr-wdc-ens-service` - E-Bill Notification Sending Service
* `bcr-wdc-shared` - Shared types and logic for the services in this repository
* `bcr-wdc-relay` - A specialized Nostr relay implementation written in Rust for the Bitcredit application.
* `bcr-wdc-demo-faucet` - A Demo Faucet for the Wildcat mint, which auto-offers bills below a certain threshold

### CI tool updates

Cargo-installed CI tools are pinned in `.github/workflows/rust.yml` and
`.github/workflows/test.yml` and use `--locked`. Update these pins in a regular PR.
Verify installation on GitHub-hosted Ubuntu with an empty `CARGO_INSTALL_ROOT`,
check the installed binary versions, then run the normal CI checks. Remove any
temporary installation-validation settings before the final review.
