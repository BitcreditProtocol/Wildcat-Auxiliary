use bcr_common::core::NodeId;
use bitflags::bitflags;
use email_address::EmailAddress;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EmailNotificationPreferences {
    pub node_id: NodeId,
    pub company_node_id: Option<NodeId>,
    pub email: EmailAddress,
    pub preferences: PreferencesFlags,
    pub token: Uuid,
}

bitflags! {
/// A set of preference flags packed in an efficient way
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
    pub struct PreferencesFlags: i64 {
        const BillSigned = 1;
        const BillAccepted = 1 << 1;
        const BillAcceptanceRequested = 1 << 2;
        const BillAcceptanceRejected = 1 << 3;
        const BillAcceptanceTimeout = 1 << 4;
        const BillAcceptanceRecourse = 1 << 5;
        const BillPaymentRequested = 1 << 6;
        const BillPaymentRejected = 1 << 7;
        const BillPaymentTimeout = 1 << 8;
        const BillPaymentRecourse = 1 << 9;
        const BillRecourseRejected = 1 << 10;
        const BillRecourseTimeout = 1 << 11;
        const BillSellOffered = 1 << 12;
        const BillBuyingRejected = 1 << 13;
        const BillPaid = 1 << 14;
        const BillRecoursePaid = 1 << 15;
        const BillEndorsed = 1 << 16;
        const BillSold = 1 << 17;
        const BillMintingRequested = 1 << 18;
        const BillNewQuote = 1 << 19;
        const BillQuoteApproved = 1 << 20;
    }
}

impl Default for PreferencesFlags {
    fn default() -> Self {
        Self::BillSigned
            | Self::BillAccepted
            | Self::BillAcceptanceRequested
            | Self::BillAcceptanceRejected
            | Self::BillAcceptanceTimeout
            | Self::BillAcceptanceRecourse
            | Self::BillPaymentRequested
            | Self::BillPaymentRejected
            | Self::BillPaymentTimeout
            | Self::BillPaymentRecourse
            | Self::BillRecourseRejected
            | Self::BillRecourseTimeout
            | Self::BillPaid
            | Self::BillRecoursePaid
            | Self::BillMintingRequested
    }
}
