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
