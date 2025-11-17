use bitcoin::{
    XOnlyPublicKey,
    hashes::{Hash, sha256},
};
use secp256k1::{Keypair, Message, SECP256K1, SecretKey, schnorr::Signature};

/// Verifies that the given string was signed using schnorr by the controller of pub_key's private key
pub fn verify_signature(
    challenge: &[u8],
    signature: &Signature,
    pub_key: &XOnlyPublicKey,
) -> Result<bool, anyhow::Error> {
    let msg = Message::from_digest_slice(challenge)?;
    Ok(SECP256K1.verify_schnorr(signature, &msg, pub_key).is_ok())
}

/// Verifies that the given payload, hashed with sha256, matches the given signature and was signed by the given key
pub fn verify_request(
    payload: &[u8],
    signature: &Signature,
    key: &XOnlyPublicKey,
) -> Result<bool, anyhow::Error> {
    let hash = sha256::Hash::hash(payload);
    let msg = Message::from_digest(*hash.as_ref());
    Ok(SECP256K1.verify_schnorr(signature, &msg, key).is_ok())
}

/// Sign the given payload, hashed with sha256, with the given key
pub fn sign_payload(req: &[u8], private_key: &SecretKey) -> Signature {
    let key_pair = Keypair::from_secret_key(SECP256K1, private_key);
    let hash: sha256::Hash = sha256::Hash::hash(req);
    let req = Message::from_digest(*hash.as_ref());

    SECP256K1.sign_schnorr(&req, &key_pair)
}

#[cfg(test)]
pub mod tests {
    use crate::wire::{EmailConfirmPayload, NotificationSendPayload};

    use super::*;
    use bcr_common::core::NodeId;
    use bitcoin::{
        base58,
        secp256k1::{Keypair, SecretKey},
    };
    use std::str::FromStr;

    pub fn signature(challenge: &[u8], private_key: &SecretKey) -> Signature {
        let key_pair = Keypair::from_secret_key(SECP256K1, private_key);
        let msg = Message::from_digest_slice(challenge).unwrap();
        SECP256K1.sign_schnorr(&msg, &key_pair)
    }

    #[test]
    fn sig_test() {
        let secret_key =
            SecretKey::from_str("8863c82829480536893fc49c4b30e244f97261e989433373d73c648c1a656a79")
                .unwrap();
        let x_only_pub = secret_key.public_key(SECP256K1).x_only_public_key().0;

        // let challenge = Challenge::new();
        let challenge = String::from("6eoK3HnhNntraJLobPBSLqAz9z6N1KNsqnFzZyyBzDRn");
        let sig = signature(&base58::decode(&challenge).unwrap(), &secret_key);
        // print to be able to manually create requests with -- --nocapture
        println!(
            "node id: {}",
            NodeId::new(secret_key.public_key(SECP256K1), bitcoin::Network::Testnet)
        );
        println!("sig: {sig}");
        let verified = verify_signature(&base58::decode(&challenge).unwrap(), &sig, &x_only_pub);
        assert!(verified.is_ok());
        assert!(verified.as_ref().unwrap());
    }

    #[test]
    fn sig_req_test() {
        let secret_key =
            SecretKey::from_str("8863c82829480536893fc49c4b30e244f97261e989433373d73c648c1a656a79")
                .unwrap();
        let x_only_pub = secret_key.public_key(SECP256K1).x_only_public_key().0;

        let req = EmailConfirmPayload {
            node_id: NodeId::new(secret_key.public_key(SECP256K1), bitcoin::Network::Testnet),
            company_node_id: None,
            confirmation_code: "432298".to_string(),
        };
        let serialized = borsh::to_vec(&req).unwrap();
        let payload = base58::encode(&serialized);

        let sig = sign_payload(&serialized, &secret_key);
        // print to be able to manually create requests with -- --nocapture
        println!("req payload: {payload}");
        println!("req sig: {sig}");
        let verified = verify_request(&serialized, &sig, &x_only_pub);
        assert!(verified.is_ok());
        assert!(verified.as_ref().unwrap());
    }

    #[test]
    fn sig_req_send_test() {
        let secret_key =
            SecretKey::from_str("8863c82829480536893fc49c4b30e244f97261e989433373d73c648c1a656a79")
                .unwrap();
        let x_only_pub = secret_key.public_key(SECP256K1).x_only_public_key().0;
        let node_id = NodeId::new(secret_key.public_key(SECP256K1), bitcoin::Network::Testnet);

        let req = NotificationSendPayload {
            kind: "BillAccepted".to_string(),
            id: "bitcrtB7nSVpa37KKGZvcz1Qz7TRRC3MvLp38FMJXbXiGaUQYt".to_string(),
            sender_node_id: node_id.clone(),
            receiver_node_id: node_id.clone(),
            receiver_company_node_id: None,
        };
        let serialized = borsh::to_vec(&req).unwrap();
        let payload = base58::encode(&serialized);

        let sig = sign_payload(&serialized, &secret_key);
        // print to be able to manually create requests with -- --nocapture
        println!("req send payload: {payload}");
        println!("req send sig: {sig}");
        let verified = verify_request(&serialized, &sig, &x_only_pub);
        assert!(verified.is_ok());
        assert!(verified.as_ref().unwrap());
    }
}
