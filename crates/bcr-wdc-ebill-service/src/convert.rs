// ----- standard library imports
use std::str::FromStr;
// ----- extra library imports
use bcr_common::wire::{bill as wire_bill, contact as wire_contact, identity as wire_identity};
use bcr_ebill_core::{
    self as ebill_core,
    application::bill as ebill_bill,
    application::contact as ebill_contact,
    application::identity as ebill_identity,
    application::notification as ebill_notification,
    protocol::{Address, City, Country, Zip},
};
use thiserror::Error;
// ----- local imports

// ----- end imports

pub(crate) type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Chrono parse {0}")]
    Chrono(#[from] chrono::ParseError),
    #[error("Url parse {0}")]
    Url(#[from] url::ParseError),
    #[error("ebill parse {0}")]
    EBill(#[from] bcr_ebill_core::application::ValidationError),
    #[error("ebill protocol {0}")]
    EBillProtocol(#[from] bcr_ebill_core::protocol::ProtocolValidationError),
}

pub(crate) fn identitytype_wire2ebill(
    input: wire_identity::IdentityType,
) -> bcr_ebill_core::protocol::blockchain::identity::IdentityType {
    match input {
        wire_identity::IdentityType::Ident => {
            bcr_ebill_core::protocol::blockchain::identity::IdentityType::Ident
        }
        wire_identity::IdentityType::Anon => {
            bcr_ebill_core::protocol::blockchain::identity::IdentityType::Anon
        }
    }
}

pub(crate) fn postaladdress_ebill2wire(
    input: ebill_core::protocol::PostalAddress,
) -> wire_identity::PostalAddress {
    wire_identity::PostalAddress {
        country: input.country.to_string(),
        city: input.city.to_string(),
        zip: input.zip.map(|z| z.to_string()),
        address: input.address.to_string(),
    }
}

pub(crate) fn optionalpostaladdress_ebill2wire(
    input: ebill_core::protocol::OptionalPostalAddress,
) -> wire_identity::OptionalPostalAddress {
    wire_identity::OptionalPostalAddress {
        country: input.country.map(|c| c.to_string()),
        city: input.city.map(|c| c.to_string()),
        zip: input.zip.map(|z| z.to_string()),
        address: input.address.map(|a| a.to_string()),
    }
}

pub(crate) fn optionalpostaladdress_wire2ebill(
    input: wire_identity::OptionalPostalAddress,
) -> Result<ebill_core::protocol::OptionalPostalAddress> {
    let output = ebill_core::protocol::OptionalPostalAddress {
        country: input.country.map(|c| Country::parse(&c)).transpose()?,
        city: input.city.map(City::new).transpose()?,
        zip: input.zip.map(|z| Zip::new(&z)).transpose()?,
        address: input.address.map(Address::new).transpose()?,
    };
    Ok(output)
}

pub(crate) fn file_ebill2wire(input: ebill_core::protocol::File) -> wire_identity::File {
    wire_identity::File {
        name: input.name.to_string(),
        hash: input.hash.to_string(),
        nostr_hash: input.nostr_hash.to_string(),
    }
}

pub(crate) fn contacttype_ebill2wire(
    input: bcr_ebill_core::protocol::blockchain::bill::ContactType,
) -> wire_contact::ContactType {
    match input {
        bcr_ebill_core::protocol::blockchain::bill::ContactType::Person => {
            wire_contact::ContactType::Person
        }
        bcr_ebill_core::protocol::blockchain::bill::ContactType::Company => {
            wire_contact::ContactType::Company
        }
        bcr_ebill_core::protocol::blockchain::bill::ContactType::Anon => {
            wire_contact::ContactType::Anon
        }
    }
}

pub(crate) fn identity_ebill2wire(
    input: ebill_identity::Identity,
) -> Result<wire_identity::Identity> {
    let output = wire_identity::Identity {
        node_id: input.node_id.clone(),
        name: input.name.to_string(),
        email: input.email.map(|e| e.to_string()),
        bitcoin_public_key: input.node_id.pub_key().into(),
        npub: input.node_id.npub(),
        postal_address: optionalpostaladdress_ebill2wire(input.postal_address),
        date_of_birth: input
            .date_of_birth
            .as_ref()
            .map(|d| d.as_str())
            .map(chrono::NaiveDate::from_str)
            .transpose()?,
        country_of_birth: input.country_of_birth.map(|c| c.to_string()),
        city_of_birth: input.city_of_birth.map(|c| c.to_string()),
        identification_number: input.identification_number.map(|i| i.to_string()),
        profile_picture_file: input.profile_picture_file.map(file_ebill2wire),
        identity_document_file: input.identity_document_file.map(file_ebill2wire),
        nostr_relays: input.nostr_relays,
    };
    Ok(output)
}

fn lightbillidentparticipantwithaddress_ebill2wire(
    input: ebill_contact::LightBillIdentParticipantWithAddress,
) -> wire_bill::LightBillIdentParticipantWithAddress {
    wire_bill::LightBillIdentParticipantWithAddress {
        t: contacttype_ebill2wire(input.t),
        name: input.name.to_string(),
        node_id: input.node_id,
        postal_address: postaladdress_ebill2wire(input.postal_address),
    }
}

fn lightbillidentparticipant_ebill2wire(
    input: ebill_contact::LightBillIdentParticipant,
) -> wire_bill::LightBillIdentParticipant {
    wire_bill::LightBillIdentParticipant {
        t: contacttype_ebill2wire(input.t),
        name: input.name.to_string(),
        node_id: input.node_id,
    }
}

fn lightbillanonparticipant_ebill2wire(
    input: ebill_contact::LightBillAnonParticipant,
) -> wire_bill::LightBillAnonParticipant {
    wire_bill::LightBillAnonParticipant {
        node_id: input.node_id,
    }
}

fn lightbillparticipant_ebill2wire(
    input: ebill_contact::LightBillParticipant,
) -> wire_bill::LightBillParticipant {
    match input {
        ebill_contact::LightBillParticipant::Ident(data) => wire_bill::LightBillParticipant::Ident(
            lightbillidentparticipantwithaddress_ebill2wire(data),
        ),
        ebill_contact::LightBillParticipant::Anon(data) => {
            wire_bill::LightBillParticipant::Anon(lightbillanonparticipant_ebill2wire(data))
        }
    }
}

fn lightsignedby_ebill2wire(input: ebill_bill::LightSignedBy) -> wire_bill::LightSignedBy {
    wire_bill::LightSignedBy {
        data: lightbillparticipant_ebill2wire(input.data),
        signatory: input.signatory.map(lightbillidentparticipant_ebill2wire),
    }
}

pub(crate) fn endorsement_ebill2wire(input: ebill_bill::Endorsement) -> wire_bill::Endorsement {
    wire_bill::Endorsement {
        pay_to_the_order_of: lightbillparticipant_ebill2wire(input.pay_to_the_order_of),
        signed: lightsignedby_ebill2wire(input.signed),
        signing_timestamp: input.signing_timestamp.inner(),
        signing_address: input.signing_address.map(postaladdress_ebill2wire),
    }
}

pub(crate) fn billcombinedbitcoinkey_ebill2wire(
    input: ebill_bill::BillCombinedBitcoinKey,
) -> wire_bill::BillCombinedBitcoinKey {
    wire_bill::BillCombinedBitcoinKey {
        private_descriptor: input.private_descriptor,
    }
}

pub(crate) fn notificationtype_ebill2wire(
    input: ebill_notification::NotificationType,
) -> wire_bill::NotificationType {
    match input {
        ebill_notification::NotificationType::Bill => wire_bill::NotificationType::Bill,
        ebill_notification::NotificationType::General => wire_bill::NotificationType::General,
        ebill_notification::NotificationType::Company => wire_bill::NotificationType::Company,
        ebill_notification::NotificationType::Contact => wire_bill::NotificationType::Contact,
    }
}

pub(crate) fn notification_ebill2wire(
    input: ebill_notification::Notification,
) -> wire_bill::Notification {
    wire_bill::Notification {
        id: input.id,
        node_id: input.node_id,
        notification_type: notificationtype_ebill2wire(input.notification_type),
        reference_id: input.reference_id,
        description: input.description,
        datetime: input.datetime,
        active: input.active,
        payload: input.payload,
    }
}

pub(crate) fn billidentparticipant_ebill2wire(
    input: bcr_ebill_core::protocol::blockchain::bill::participant::BillIdentParticipant,
) -> wire_bill::BillIdentParticipant {
    wire_bill::BillIdentParticipant {
        t: contacttype_ebill2wire(input.t),
        name: input.name.to_string(),
        node_id: input.node_id,
        postal_address: postaladdress_ebill2wire(input.postal_address),
        email: input.email.map(|e| e.to_string()),
        nostr_relays: input.nostr_relays,
    }
}

pub(crate) fn billanonparticipant_ebill2wire(
    input: bcr_ebill_core::protocol::blockchain::bill::participant::BillAnonParticipant,
) -> wire_bill::BillAnonParticipant {
    wire_bill::BillAnonParticipant {
        node_id: input.node_id,
        nostr_relays: input.nostr_relays,
    }
}

pub(crate) fn billparticipant_ebill2wire(
    input: bcr_ebill_core::protocol::blockchain::bill::participant::BillParticipant,
) -> wire_bill::BillParticipant {
    match input {
        bcr_ebill_core::protocol::blockchain::bill::participant::BillParticipant::Ident(data) => {
            wire_bill::BillParticipant::Ident(billidentparticipant_ebill2wire(data))
        }
        bcr_ebill_core::protocol::blockchain::bill::participant::BillParticipant::Anon(data) => {
            wire_bill::BillParticipant::Anon(billanonparticipant_ebill2wire(data))
        }
    }
}

pub(crate) fn billparticipants_ebill2wire(
    input: bcr_ebill_core::application::bill::BillParticipants,
) -> wire_bill::BillParticipants {
    wire_bill::BillParticipants {
        drawee: billidentparticipant_ebill2wire(input.drawee),
        drawer: billidentparticipant_ebill2wire(input.drawer),
        payee: billparticipant_ebill2wire(input.payee),
        endorsee: input.endorsee.map(billparticipant_ebill2wire),
        endorsements_count: input.endorsements_count,
        all_participant_node_ids: input.all_participant_node_ids,
        endorsements: input
            .endorsements
            .into_iter()
            .map(endorsement_ebill2wire)
            .collect(),
    }
}

pub(crate) fn billdata_ebill2wire(input: ebill_bill::BillData) -> Result<wire_bill::BillData> {
    let issue_date = chrono::NaiveDate::from_str(input.issue_date.as_str())?;
    let maturity_date = chrono::NaiveDate::from_str(input.maturity_date.as_str())?;
    let output = wire_bill::BillData {
        time_of_drawing: input.time_of_drawing.inner(),
        issue_date,
        time_of_maturity: input.time_of_maturity.inner(),
        maturity_date,
        country_of_issuing: input.country_of_issuing.to_string(),
        city_of_issuing: input.city_of_issuing.to_string(),
        country_of_payment: input.country_of_payment.to_string(),
        city_of_payment: input.city_of_payment.to_string(),
        currency: input.sum.currency().code().to_owned(),
        sum: input.sum.as_sat_string(),
        files: input.files.into_iter().map(file_ebill2wire).collect(),
        active_notification: input.active_notification.map(notification_ebill2wire),
    };
    Ok(output)
}

pub(crate) fn billpaymentstatus_ebill2wire(
    input: bcr_ebill_core::application::bill::BillPaymentStatus,
) -> wire_bill::BillPaymentStatus {
    wire_bill::BillPaymentStatus {
        rejected_to_pay: input.rejected_to_pay,
        requested_to_pay: input.requested_to_pay,
        request_to_pay_timed_out: input.request_to_pay_timed_out,
        time_of_request_to_pay: input.time_of_request_to_pay.map(|t| t.inner()),
        paid: input.paid,
        payment_deadline_timestamp: input.payment_deadline_timestamp.map(|t| t.inner()),
    }
}

pub(crate) fn billstatus_ebill2wire(
    input: bcr_ebill_core::application::bill::BillStatus,
) -> wire_bill::BillStatus {
    let acceptance = wire_bill::BillAcceptanceStatus {
        time_of_request_to_accept: input
            .acceptance
            .time_of_request_to_accept
            .map(|t| t.inner()),
        accepted: input.acceptance.accepted,
        rejected_to_accept: input.acceptance.rejected_to_accept,
        requested_to_accept: input.acceptance.requested_to_accept,
        request_to_accept_timed_out: input.acceptance.request_to_accept_timed_out,
        acceptance_deadline_timestamp: input
            .acceptance
            .acceptance_deadline_timestamp
            .map(|t| t.inner()),
    };
    let payment = billpaymentstatus_ebill2wire(input.payment);
    let sell = wire_bill::BillSellStatus {
        offered_to_sell: input.sell.offered_to_sell,
        offer_to_sell_timed_out: input.sell.offer_to_sell_timed_out,
        rejected_offer_to_sell: input.sell.rejected_offer_to_sell,
        sold: input.sell.sold,
        time_of_last_offer_to_sell: input.sell.time_of_last_offer_to_sell.map(|t| t.inner()),
        buying_deadline_timestamp: input.sell.buying_deadline_timestamp.map(|t| t.inner()),
    };
    let recourse = wire_bill::BillRecourseStatus {
        recoursed: input.recourse.recoursed,
        requested_to_recourse: input.recourse.requested_to_recourse,
        request_to_recourse_timed_out: input.recourse.request_to_recourse_timed_out,
        rejected_request_to_recourse: input.recourse.rejected_request_to_recourse,
        time_of_last_request_to_recourse: input
            .recourse
            .time_of_last_request_to_recourse
            .map(|t| t.inner()),
        recourse_deadline_timestamp: input
            .recourse
            .recourse_deadline_timestamp
            .map(|t| t.inner()),
    };
    wire_bill::BillStatus {
        acceptance,
        payment,
        sell,
        recourse,
        redeemed_funds_available: input.redeemed_funds_available,
        has_requested_funds: input.has_requested_funds,
        mint: billmintstatus_ebill2wire(input.mint),
        last_block_time: input.last_block_time.inner(),
    }
}

fn billmintstatus_ebill2wire(
    input: bcr_ebill_core::application::bill::BillMintStatus,
) -> wire_bill::BillMintStatus {
    wire_bill::BillMintStatus {
        has_mint_requests: input.has_mint_requests,
    }
}

pub(crate) fn billwaitingforpaymentstate_ebill2wire(
    input: bcr_ebill_core::application::bill::BillWaitingForPaymentState,
) -> wire_bill::BillWaitingForPaymentState {
    wire_bill::BillWaitingForPaymentState {
        payee: billparticipant_ebill2wire(input.payee),
        payer: billidentparticipant_ebill2wire(input.payer),
        payment_data: billwaitingstatepaymentdata_ebill2wire(input.payment_data),
    }
}

pub(crate) fn billwaitingstatepaymentdata_ebill2wire(
    input: bcr_ebill_core::application::bill::BillWaitingStatePaymentData,
) -> wire_bill::BillWaitingStatePaymentData {
    wire_bill::BillWaitingStatePaymentData {
        address_to_pay: input.address_to_pay.assume_checked().to_string(),
        currency: input.sum.currency().code().to_owned(),
        link_to_pay: input.link_to_pay,
        mempool_link_for_address_to_pay: input.mempool_link_for_address_to_pay,
        time_of_request: input.time_of_request.inner(),
        sum: input.sum.as_sat_string(),
        confirmations: input.confirmations,
        in_mempool: input.in_mempool,
        payment_deadline: input.payment_deadline.map(|t| t.inner()),
        tx_id: input.tx_id,
    }
}

pub(crate) fn billcurrentwaitingstate_ebill2wire(
    input: bcr_ebill_core::application::bill::BillCurrentWaitingState,
) -> wire_bill::BillCurrentWaitingState {
    match input {
        ebill_bill::BillCurrentWaitingState::Sell(state) => {
            let state = wire_bill::BillWaitingForSellState {
                buyer: billparticipant_ebill2wire(state.buyer),
                seller: billparticipant_ebill2wire(state.seller),
                payment_data: billwaitingstatepaymentdata_ebill2wire(state.payment_data),
            };
            wire_bill::BillCurrentWaitingState::Sell(state)
        }
        ebill_bill::BillCurrentWaitingState::Payment(state) => {
            let state = billwaitingforpaymentstate_ebill2wire(state);
            wire_bill::BillCurrentWaitingState::Payment(state)
        }
        ebill_bill::BillCurrentWaitingState::Recourse(state) => {
            let state = wire_bill::BillWaitingForRecourseState {
                recourser: billparticipant_ebill2wire(state.recourser),
                recoursee: billidentparticipant_ebill2wire(state.recoursee),
                payment_data: billwaitingstatepaymentdata_ebill2wire(state.payment_data),
            };
            wire_bill::BillCurrentWaitingState::Recourse(state)
        }
    }
}

pub(crate) fn bitcreditbill_ebill2wire(
    input: bcr_ebill_core::application::bill::BitcreditBillResult,
) -> Result<wire_bill::BitcreditBill> {
    let output = wire_bill::BitcreditBill {
        id: input.id,
        participants: billparticipants_ebill2wire(input.participants),
        data: billdata_ebill2wire(input.data)?,
        status: billstatus_ebill2wire(input.status),
        current_waiting_state: input
            .current_waiting_state
            .map(billcurrentwaitingstate_ebill2wire),
    };
    Ok(output)
}
