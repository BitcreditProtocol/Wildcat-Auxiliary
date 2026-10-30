# 0.5.1

* Add additional rate-limiting configs for bcr-wdc-relay: `queries_per_minute`, `messages_per_minute` and `max_filter_limit`
* eBills service retries failed Nostr consumer starts and task exits with bounded backoff, and exposes consumer readiness at `/health`.

# 0.5.0

* eBills service upgraded to E-Bill 0.6.0 storage moves from SurrealDB to Postgres.
* New eBills endpoints: bill balance history (counts unpaid bills only) and `check_bill_payment`.
* Admin notifications (ens-service): privileged admin sources can now send email notifications to a configured list of addresses, with no authentication
  needed.
* SQLx/Postgres persistence: eic-service, ens-service and the shared challenge repository now have SQLx backends. eic-service gets a migration script.

# 0.3.0

* Upgraded E-Bill-Service to E-Bill 0.6.0
    * This includes a database migration from SurrealDB to Postgres
        * Migration can be done by
            * Deploying the service
                * `create_identity_if_none_exists` should be set to `false` (the default), so no identity is created by default
            * Call `PUT /v1/admin/identity/seed/recover` with the correct `seed_phrase`
                * This will start the recovery process
            * Once restoration is done, check `/v1/admin/identity/detail` if everything is correct
            * Restart the pod/service (important! otherwise, the nostr connection isn't properly set up)
* Add flag `appcfg.create_identity_if_none_exists` to control if an identity should be created by default, if none exists
    * If it's a new, fresh instance (e.g. e2e tests, local setup, new cluster) - this should be set to `true`
    * Otherwise, if the instance is supposed to be recovered from a seed phrase, it should be set to the default `false`

# 0.2.0

## bcr-wdc-demo-faucet

* Fix one failed deny/offer/lookup cancelling the whole loop

## bcr-ebill-service

* Add endpoint for `get_bills_balance_history` - to return a balance overview for bills
* Add endpoint for `check_bill_payment` to check a bill's payment

## all services

* Upgrade Dependencies
* Migrate `chrono` to `time`

## bcr-wdc-relay

* Add `relay_url` config parameter (ENV: `RELAY_URL`), which must be set to the public facing URL of the relay

# 0.1.0

## bcr-wdc-eic-service

* First Implementation

## bcr-wdc-ens-service

* First Implementation
