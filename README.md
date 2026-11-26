# Wildcat-Auxiliary

Wildcat auxiliary services

### Crates

The project consists of the following crates:

* `bcr-wdc-ebill-service` - eBills Service
* `bcr-wdc-eic-service` - eBills Identity Confirmation Service
* `bcr-wdc-ens-service` - eBills Notification Sending Service
* `bcr-wdc-shared` - Shared types and logic for the services in this repository
* `bcr-wdc-relay` - A specialized Nostr relay implementation written in Rust for the Bitcredit application.
* `bcr-wdc-demo-faucet` - A Demo Faucet for the Wildcat mint, which auto-offers bills below a certain threshold

### eBills service health

`GET /health` returns `200 OK` while the Nostr consumer task is running, and
`503 Service Unavailable` while it is starting or recovering. This reports consumer
readiness, not connectivity to every relay or successful delivery of bill events.

Failed starts and unexpected task exits are retried with exponential backoff from
1 to 60 seconds. The existing periodic resubscription is retained to recover missed
events: `restart_nostr_consumer_interval_secs` defaults to 240 seconds, with a minimum
of 1 second.
