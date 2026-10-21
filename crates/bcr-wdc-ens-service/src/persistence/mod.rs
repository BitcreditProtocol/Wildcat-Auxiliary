// not wired into the service yet: only reached by the tests
#[allow(dead_code)]
pub mod sql;
pub mod surreal;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        email_preferences::PreferencesFlags, service::EmailNotificationPreferencesRepository,
    };
    use bcr_common::core::NodeId;
    use bitcoin::{
        Network,
        secp256k1::{SECP256K1, SecretKey},
    };
    use email_address::EmailAddress;
    use std::str::FromStr;
    use uuid::Uuid;

    async fn init_surreal_db() -> surreal::DBEmailNotificationPreferences {
        surreal::DBEmailNotificationPreferences::new(surreal::ConnectionConfig {
            connection: String::from("mem://"),
            namespace: String::from("test"),
            database: String::from("test"),
            table: String::from("email_notification_preferences"),
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

    #[tokio::test]
    async fn test_prefs_get_missing() {
        let db = init_surreal_db().await;
        prefs_get_missing(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_get_missing_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_get_missing(db).await;
    }
    async fn prefs_get_missing(db: impl EmailNotificationPreferencesRepository) {
        assert!(
            db.get_preferences_for_node_id(&random_node_id(), &None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.get_preferences_for_token(&Uuid::new_v4())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_prefs_set_and_get() {
        let db = init_surreal_db().await;
        prefs_set_and_get(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_set_and_get_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_set_and_get(db).await;
    }
    async fn prefs_set_and_get(db: impl EmailNotificationPreferencesRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        let token = Uuid::new_v4();
        let flags = PreferencesFlags::BillSigned | PreferencesFlags::BillPaid;
        db.set_preferences_for_node_id(&node_id, &company, &email("a@b.cr"), false, flags, &token)
            .await
            .unwrap();
        let res = db
            .get_preferences_for_node_id(&node_id, &company)
            .await
            .unwrap()
            .expect("preferences");
        assert_eq!(res.node_id, node_id);
        assert_eq!(res.company_node_id, company);
        assert_eq!(res.email, email("a@b.cr"));
        assert!(!res.enabled);
        assert_eq!(res.preferences, flags);
        assert_eq!(res.pref_token, token);
    }

    #[tokio::test]
    async fn test_prefs_company_key_is_distinct() {
        let db = init_surreal_db().await;
        prefs_company_key_is_distinct(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_company_key_is_distinct_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_company_key_is_distinct(db).await;
    }
    async fn prefs_company_key_is_distinct(db: impl EmailNotificationPreferencesRepository) {
        let node_id = random_node_id();
        let company = Some(random_node_id());
        let personal_token = Uuid::new_v4();
        let company_token = Uuid::new_v4();
        db.set_preferences_for_node_id(
            &node_id,
            &None,
            &email("a@b.cr"),
            true,
            PreferencesFlags::default(),
            &personal_token,
        )
        .await
        .unwrap();
        db.set_preferences_for_node_id(
            &node_id,
            &company,
            &email("c@d.cr"),
            true,
            PreferencesFlags::default(),
            &company_token,
        )
        .await
        .unwrap();
        let personal = db
            .get_preferences_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("personal");
        let corporate = db
            .get_preferences_for_node_id(&node_id, &company)
            .await
            .unwrap()
            .expect("company");
        assert_eq!(personal.email, email("a@b.cr"));
        assert_eq!(personal.company_node_id, None);
        assert_eq!(personal.pref_token, personal_token);
        assert_eq!(corporate.email, email("c@d.cr"));
        assert_eq!(corporate.company_node_id, company);
        assert_eq!(corporate.pref_token, company_token);
    }

    #[tokio::test]
    async fn test_prefs_set_overwrites() {
        let db = init_surreal_db().await;
        prefs_set_overwrites(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_set_overwrites_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_set_overwrites(db).await;
    }
    async fn prefs_set_overwrites(db: impl EmailNotificationPreferencesRepository) {
        let node_id = random_node_id();
        let old_token = Uuid::new_v4();
        db.set_preferences_for_node_id(
            &node_id,
            &None,
            &email("a@b.cr"),
            true,
            PreferencesFlags::default(),
            &old_token,
        )
        .await
        .unwrap();
        let new_token = Uuid::new_v4();
        db.set_preferences_for_node_id(
            &node_id,
            &None,
            &email("c@d.cr"),
            false,
            PreferencesFlags::BillSold,
            &new_token,
        )
        .await
        .unwrap();
        let res = db
            .get_preferences_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("preferences");
        assert_eq!(res.email, email("c@d.cr"));
        assert!(!res.enabled);
        assert_eq!(res.preferences, PreferencesFlags::BillSold);
        assert_eq!(res.pref_token, new_token);
        assert!(
            db.get_preferences_for_token(&old_token)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_prefs_get_for_token() {
        let db = init_surreal_db().await;
        prefs_get_for_token(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_get_for_token_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_get_for_token(db).await;
    }
    async fn prefs_get_for_token(db: impl EmailNotificationPreferencesRepository) {
        let first = random_node_id();
        let second = random_node_id();
        let first_token = Uuid::new_v4();
        let second_token = Uuid::new_v4();
        db.set_preferences_for_node_id(
            &first,
            &None,
            &email("a@b.cr"),
            true,
            PreferencesFlags::default(),
            &first_token,
        )
        .await
        .unwrap();
        db.set_preferences_for_node_id(
            &second,
            &None,
            &email("c@d.cr"),
            true,
            PreferencesFlags::default(),
            &second_token,
        )
        .await
        .unwrap();
        let res = db
            .get_preferences_for_token(&second_token)
            .await
            .unwrap()
            .expect("preferences");
        assert_eq!(res.node_id, second);
        assert_eq!(res.email, email("c@d.cr"));
        assert_eq!(res.pref_token, second_token);
    }

    #[tokio::test]
    async fn test_prefs_update_for_token() {
        let db = init_surreal_db().await;
        prefs_update_for_token(db).await;
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires DATABASE_URL with CREATEDB permission"]
    async fn test_prefs_update_for_token_sql(pool: ::sqlx::PgPool) {
        let db = sql::DB::from_pool(pool);
        prefs_update_for_token(db).await;
    }
    async fn prefs_update_for_token(db: impl EmailNotificationPreferencesRepository) {
        let node_id = random_node_id();
        let other = random_node_id();
        let token = Uuid::new_v4();
        let other_token = Uuid::new_v4();
        db.set_preferences_for_node_id(
            &node_id,
            &None,
            &email("a@b.cr"),
            true,
            PreferencesFlags::default(),
            &token,
        )
        .await
        .unwrap();
        db.set_preferences_for_node_id(
            &other,
            &None,
            &email("c@d.cr"),
            true,
            PreferencesFlags::default(),
            &other_token,
        )
        .await
        .unwrap();
        db.update_prefences_for_token(&token, false, PreferencesFlags::BillPaid)
            .await
            .unwrap();
        let res = db
            .get_preferences_for_node_id(&node_id, &None)
            .await
            .unwrap()
            .expect("preferences");
        assert!(!res.enabled);
        assert_eq!(res.preferences, PreferencesFlags::BillPaid);
        assert_eq!(res.email, email("a@b.cr"));
        assert_eq!(res.pref_token, token);
        let untouched = db
            .get_preferences_for_node_id(&other, &None)
            .await
            .unwrap()
            .expect("other");
        assert!(untouched.enabled);
        assert_eq!(untouched.preferences, PreferencesFlags::default());
        // unknown token: no error, nothing created
        let unknown = Uuid::new_v4();
        db.update_prefences_for_token(&unknown, false, PreferencesFlags::empty())
            .await
            .unwrap();
        assert!(
            db.get_preferences_for_token(&unknown)
                .await
                .unwrap()
                .is_none()
        );
    }
}
