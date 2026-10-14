pub mod surreal;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::EmailConfirmationRepository;
    use bcr_common::core::NodeId;
    use bitcoin::{
        Network,
        secp256k1::{SECP256K1, SecretKey},
    };
    use email_address::EmailAddress;
    use std::str::FromStr;

    async fn init_surreal_db() -> surreal::DBEmails {
        surreal::DBEmails::new(surreal::EmailConnectionConfig {
            connection: String::from("mem://"),
            namespace: String::from("test"),
            database: String::from("test"),
            table_confirmations: String::from("email_confirmations"),
            table_registrations: String::from("email_registrations"),
        })
        .await
        .expect("mem db")
    }

    fn random_node_id() -> NodeId {
        let sk = SecretKey::new(&mut bitcoin::secp256k1::rand::thread_rng());
        NodeId::new(sk.public_key(SECP256K1), Network::Testnet)
    }

    fn email(s: &str) -> EmailAddress {
        EmailAddress::from_str(s).unwrap()
    }

    fn random_signature() -> bitcoin::secp256k1::schnorr::Signature {
        let sk = SecretKey::new(&mut bitcoin::secp256k1::rand::thread_rng());
        bcr_wdc_shared::signature::sign_payload(b"payload", &sk)
    }

    #[tokio::test]
    async fn test_confirmations_insert_and_get() {
        confirmations_insert_and_get(init_surreal_db().await).await;
    }
    async fn confirmations_insert_and_get(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        assert!(
            db.get_confirmation_email_for_node_id(&node_id, &None)
                .await
                .unwrap()
                .is_none()
        );
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("a@b.cr"), "123456")
            .await
            .unwrap();
        let res = db
            .get_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("confirmation");
        assert_eq!(res.node_id, node_id);
        assert_eq!(res.company_node_id, None);
        assert_eq!(res.email, email("a@b.cr"));
        assert_eq!(res.confirmation_code, "123456");
        assert_eq!(res.wrong_entries, 0);
        assert!(!res.is_expired());
    }

    #[tokio::test]
    async fn test_confirmations_company_key_is_distinct() {
        confirmations_company_key_is_distinct(init_surreal_db().await).await;
    }
    async fn confirmations_company_key_is_distinct(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        db.insert_confirmation_email_for_node_id(&node_id, &company, &email("c@d.cr"), "222222")
            .await
            .unwrap();
        let personal = db
            .get_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("personal");
        let corporate = db
            .get_confirmation_email_for_node_id(&node_id, &company)
            .await
            .unwrap()
            .expect("company");
        assert_eq!(personal.confirmation_code, "111111");
        assert_eq!(corporate.confirmation_code, "222222");
        assert_eq!(corporate.company_node_id, company);
    }

    #[tokio::test]
    async fn test_confirmations_insert_overwrites_and_resets_wrong_entries() {
        confirmations_insert_overwrites_and_resets_wrong_entries(init_surreal_db().await).await;
    }
    async fn confirmations_insert_overwrites_and_resets_wrong_entries(
        db: impl EmailConfirmationRepository,
    ) {
        let node_id = random_node_id();
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        db.record_wrong_entry_for_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap();
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("c@d.cr"), "222222")
            .await
            .unwrap();
        let res = db
            .get_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("confirmation");
        assert_eq!(res.email, email("c@d.cr"));
        assert_eq!(res.confirmation_code, "222222");
        assert_eq!(res.wrong_entries, 0);
    }

    #[tokio::test]
    async fn test_confirmations_record_wrong_entry() {
        confirmations_record_wrong_entry(init_surreal_db().await).await;
    }
    async fn confirmations_record_wrong_entry(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        db.insert_confirmation_email_for_node_id(&node_id, &company, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        for _ in 0..3 {
            db.record_wrong_entry_for_confirmation_email_for_node_id(&node_id, &None)
                .await
                .unwrap();
        }
        let personal = db
            .get_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("personal");
        let corporate = db
            .get_confirmation_email_for_node_id(&node_id, &company)
            .await
            .unwrap()
            .expect("company");
        assert_eq!(personal.wrong_entries, 3);
        assert_eq!(corporate.wrong_entries, 0);
        // missing confirmation: no error, nothing created
        let other = random_node_id();
        db.record_wrong_entry_for_confirmation_email_for_node_id(&other, &None)
            .await
            .unwrap();
        assert!(
            db.get_confirmation_email_for_node_id(&other, &None)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_confirmations_remove() {
        confirmations_remove(init_surreal_db().await).await;
    }
    async fn confirmations_remove(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        db.insert_confirmation_email_for_node_id(&node_id, &None, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        db.insert_confirmation_email_for_node_id(&node_id, &company, &email("a@b.cr"), "111111")
            .await
            .unwrap();
        db.remove_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap();
        assert!(
            db.get_confirmation_email_for_node_id(&node_id, &None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.get_confirmation_email_for_node_id(&node_id, &company)
                .await
                .unwrap()
                .is_some()
        );
        // removing a missing confirmation is not an error
        db.remove_confirmation_email_for_node_id(&node_id, &None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_registrations_insert_and_get() {
        registrations_insert_and_get(init_surreal_db().await).await;
    }
    async fn registrations_insert_and_get(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        let signature = random_signature();
        assert!(
            db.get_email_registration_for_node_id(&node_id, &company)
                .await
                .unwrap()
                .is_none()
        );
        db.insert_email_registration_for_node_id(&node_id, &company, &email("a@b.cr"), &signature)
            .await
            .unwrap();
        let res = db
            .get_email_registration_for_node_id(&node_id, &company)
            .await
            .unwrap()
            .expect("registration");
        assert_eq!(res.node_id, node_id);
        assert_eq!(res.company_node_id, company);
        assert_eq!(res.email, email("a@b.cr"));
        assert_eq!(res.mint_signature, signature);
        assert!(
            db.get_email_registration_for_node_id(&node_id, &None)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_registrations_insert_overwrites() {
        registrations_insert_overwrites(init_surreal_db().await).await;
    }
    async fn registrations_insert_overwrites(db: impl EmailConfirmationRepository) {
        let node_id = random_node_id();
        db.insert_email_registration_for_node_id(
            &node_id,
            &None,
            &email("a@b.cr"),
            &random_signature(),
        )
        .await
        .unwrap();
        let signature = random_signature();
        db.insert_email_registration_for_node_id(&node_id, &None, &email("c@d.cr"), &signature)
            .await
            .unwrap();
        let res = db
            .get_email_registration_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("registration");
        assert_eq!(res.email, email("c@d.cr"));
        assert_eq!(res.mint_signature, signature);
    }
}
