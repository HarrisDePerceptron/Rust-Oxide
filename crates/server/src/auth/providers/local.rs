use async_trait::async_trait;

use crate::{
    auth::{
        Claims, Role, TokenBundle,
        jwt::{JwtKeys, encode_token, make_access_claims},
        password::{hash_password, verify_password},
    },
    config::AuthConfig,
    db::dao::RefreshTokenDao,
    db::entities,
    error::AppError,
    services::user_service::UserService,
};

use super::{AuthProvider, AuthProviderId};

const ACCESS_TTL_SECS: usize = 15 * 60; // 15 minutes
const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Clone)]
pub struct LocalAuthProvider {
    user_service: UserService,
    refresh_token_dao: RefreshTokenDao,
    jwt: JwtKeys,
}

impl LocalAuthProvider {
    pub fn new(
        user_service: UserService,
        refresh_token_dao: RefreshTokenDao,
        jwt: JwtKeys,
    ) -> Self {
        Self {
            user_service,
            refresh_token_dao,
            jwt,
        }
    }

    async fn issue_tokens(&self, user: &entities::user::Model) -> Result<TokenBundle, AppError> {
        let primary_role = Role::try_from(user.role.as_str()).unwrap_or(Role::User);
        let mut roles = vec![primary_role.clone()];
        if matches!(primary_role, Role::Admin) {
            roles.push(Role::User);
        }
        let claims = make_access_claims(&user.id, roles, ACCESS_TTL_SECS);
        let access_token = encode_token(&self.jwt, &claims)?;

        let refresh = self
            .refresh_token_dao
            .create_refresh_token(&user.id, Some(REFRESH_TTL_DAYS))
            .await?;

        Ok(TokenBundle {
            access_token,
            refresh_token: refresh.token,
            token_type: "Bearer",
            expires_in: ACCESS_TTL_SECS,
        })
    }
}

#[async_trait]
impl AuthProvider for LocalAuthProvider {
    fn id(&self) -> AuthProviderId {
        AuthProviderId::Local
    }

    async fn register(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(AppError::bad_request("Email required"));
        }

        if self.user_service.find_by_email(email).await?.is_some() {
            return Err(AppError::conflict("User already exists"));
        }

        let password_hash = hash_password(password)?;
        let user = self
            .user_service
            .create_user(email, &password_hash, Role::User.as_str())
            .await?;

        self.issue_tokens(&user).await
    }

    async fn login(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        let user = self
            .user_service
            .find_by_email(email)
            .await?
            .ok_or_else(|| AppError::unauthorized("Invalid credentials"))?;

        let password_ok = verify_password(password, &user.password_hash)?;
        if !password_ok {
            return Err(AppError::unauthorized("Invalid credentials"));
        }

        let now = chrono::Utc::now().fixed_offset();
        self.user_service.set_last_login(&user.id, &now).await?;

        self.issue_tokens(&user).await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, AppError> {
        let token = self
            .refresh_token_dao
            .find_active_by_token(refresh_token)
            .await?
            .ok_or_else(|| AppError::unauthorized("Invalid refresh token"))?;

        if token.expires_at < chrono::Utc::now().fixed_offset() || token.revoked {
            return Err(AppError::unauthorized("Refresh token expired"));
        }

        let user = self
            .user_service
            .find_by_id(&token.user_id)
            .await?
            .ok_or_else(|| AppError::unauthorized("Invalid refresh token"))?;

        self.refresh_token_dao.revoke_token(refresh_token).await?;

        self.issue_tokens(&user).await
    }

    async fn verify(&self, access_token: &str) -> Result<Claims, AppError> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.validate_exp = true;
        let data = jsonwebtoken::decode::<Claims>(access_token, &self.jwt.dec, &validation)?;
        Ok(data.claims)
    }

    async fn seed_admin(&self, cfg: &AuthConfig) -> anyhow::Result<()> {
        if let Some(existing) = self
            .user_service
            .find_by_email(&cfg.admin_email)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
        {
            tracing::info!("admin user already present: {}", existing.email);
            return Ok(());
        }

        let hash = hash_password(&cfg.admin_password)
            .map_err(|e| anyhow::anyhow!("admin seed hash error: {e}"))?;
        let user = self
            .user_service
            .create_user(&cfg.admin_email, &hash, Role::Admin.as_str())
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        tracing::info!("seeded admin user {}", user.email);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, FixedOffset, TimeZone, Utc};
    use sea_orm::{DatabaseBackend, DbErr, IntoMockRow, MockDatabase, MockExecResult};
    use uuid::Uuid;

    use crate::{
        auth::{
            Role,
            jwt::{encode_token, make_access_claims},
            password::hash_password,
            providers::AuthProvider,
        },
        config::AuthConfig,
        db::entities::{refresh_token, user},
        services::ServiceContext,
    };

    use super::{ACCESS_TTL_SECS, AuthProviderId, LocalAuthProvider};

    struct ProviderFixtureBuilder {
        mock: MockDatabase,
        secret: Vec<u8>,
    }

    impl ProviderFixtureBuilder {
        fn new() -> Self {
            Self {
                mock: MockDatabase::new(DatabaseBackend::Postgres),
                secret: b"test-secret".to_vec(),
            }
        }

        fn with_secret(mut self, secret: &[u8]) -> Self {
            self.secret = secret.to_vec();
            self
        }

        fn with_query_results<T, I, II>(mut self, sets: II) -> Self
        where
            T: IntoMockRow,
            I: IntoIterator<Item = T>,
            II: IntoIterator<Item = I>,
        {
            self.mock = self.mock.append_query_results(sets);
            self
        }

        fn with_query_error(mut self, error: DbErr) -> Self {
            self.mock = self.mock.append_query_errors([error]);
            self
        }

        fn with_exec_result(mut self, rows_affected: u64) -> Self {
            self.mock = self.mock.append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected,
            }]);
            self
        }

        fn build(self) -> LocalAuthProvider {
            let db = self.mock.into_connection();
            let services = ServiceContext::new(&db);
            LocalAuthProvider::new(
                services.user(),
                services.refresh_token_dao(),
                crate::auth::jwt::JwtKeys::from_secret(&self.secret),
            )
        }
    }

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn user_model(id: Uuid, email: &str, password_hash: &str, role: &str) -> user::Model {
        user::Model {
            id,
            created_at: ts(),
            updated_at: ts(),
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            role: role.to_string(),
            last_login_at: None,
        }
    }

    fn refresh_token_model(
        token: &str,
        user_id: Uuid,
        expires_at: chrono::DateTime<chrono::FixedOffset>,
        revoked: bool,
    ) -> refresh_token::Model {
        refresh_token::Model {
            id: Uuid::new_v4(),
            created_at: ts(),
            updated_at: ts(),
            token: token.to_string(),
            user_id,
            expires_at,
            revoked,
        }
    }

    fn test_config(admin_email: &str, admin_password: &str) -> AuthConfig {
        AuthConfig {
            provider: AuthProviderId::Local,
            jwt_secret: "unit-test-secret".to_string(),
            admin_email: admin_email.to_string(),
            admin_password: admin_password.to_string(),
        }
    }

    #[tokio::test]
    async fn provider_id_is_local() {
        let provider = ProviderFixtureBuilder::new().build();

        assert_eq!(provider.id(), AuthProviderId::Local);
    }

    #[tokio::test]
    async fn verify_accepts_valid_token() {
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"verify-secret")
            .build();
        let claims = make_access_claims(&Uuid::new_v4(), vec![Role::User], 300);
        let token = encode_token(
            &crate::auth::jwt::JwtKeys::from_secret(b"verify-secret"),
            &claims,
        )
        .expect("token should encode");

        let verified = provider
            .verify(&token)
            .await
            .expect("verify should succeed");

        assert_eq!(verified.sub, claims.sub);
    }

    #[tokio::test]
    async fn verify_rejects_invalid_token() {
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"verify-secret")
            .build();

        let err = provider
            .verify("not-a-jwt")
            .await
            .expect_err("verify should fail");

        assert!(err.message().starts_with("Invalid or expired token:"));
    }

    #[tokio::test]
    async fn verify_rejects_token_signed_with_different_secret() {
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"provider-secret-a")
            .build();
        let claims = make_access_claims(&Uuid::new_v4(), vec![Role::User], 300);
        let token = encode_token(
            &crate::auth::jwt::JwtKeys::from_secret(b"provider-secret-b"),
            &claims,
        )
        .expect("token should encode");

        let err = provider
            .verify(&token)
            .await
            .expect_err("verify should fail for mismatched secret");

        assert!(err.message().starts_with("Invalid or expired token:"));
    }

    #[tokio::test]
    async fn register_rejects_whitespace_email() {
        let provider = ProviderFixtureBuilder::new().build();

        let err = provider
            .register("   ", "password123")
            .await
            .expect_err("register should fail");

        assert_eq!(err.message(), "Email required");
    }

    #[tokio::test]
    async fn register_rejects_existing_user() {
        let existing_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![user_model(
                existing_id,
                "alice@example.com",
                "hash",
                "user",
            )]])
            .build();

        let err = provider
            .register("alice@example.com", "password123")
            .await
            .expect_err("register should fail");

        assert_eq!(err.message(), "User already exists");
    }

    #[tokio::test]
    async fn register_rejects_short_password() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .build();

        let err = provider
            .register("alice@example.com", "short")
            .await
            .expect_err("register should fail");

        assert_eq!(err.message(), "Password too short");
    }

    #[tokio::test]
    async fn register_returns_token_bundle_on_success() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"register-secret")
            .with_query_results([Vec::<user::Model>::new()])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                "hashed-password",
                "user",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-register-1",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .register("alice@example.com", "password123")
            .await
            .expect("register should succeed");

        assert_eq!(bundle.refresh_token, "refresh-register-1");
    }

    #[tokio::test]
    async fn register_issues_user_role_claim() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"register-role-secret")
            .with_query_results([Vec::<user::Model>::new()])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                "hashed-password",
                "user",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-register-roles",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .register("alice@example.com", "password123")
            .await
            .expect("register should succeed");
        let claims = provider
            .verify(&bundle.access_token)
            .await
            .expect("token should verify");

        assert_eq!(claims.roles, vec![Role::User]);
    }

    #[tokio::test]
    async fn login_rejects_missing_user() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .build();

        let err = provider
            .login("alice@example.com", "password123")
            .await
            .expect_err("login should fail");

        assert_eq!(err.message(), "Invalid credentials");
    }

    #[tokio::test]
    async fn login_rejects_wrong_password() {
        let password_hash = hash_password("correct-password").expect("hash should succeed");
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![user_model(
                Uuid::new_v4(),
                "alice@example.com",
                &password_hash,
                "user",
            )]])
            .build();

        let err = provider
            .login("alice@example.com", "wrong-password")
            .await
            .expect_err("login should fail");

        assert_eq!(err.message(), "Invalid credentials");
    }

    #[tokio::test]
    async fn login_rejects_invalid_stored_hash() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![user_model(
                Uuid::new_v4(),
                "alice@example.com",
                "not-a-valid-hash",
                "user",
            )]])
            .build();

        let err = provider
            .login("alice@example.com", "password123")
            .await
            .expect_err("login should fail");

        assert!(err.message().starts_with("Invalid password hash:"));
    }

    #[tokio::test]
    async fn login_returns_token_bundle_on_success() {
        let user_id = Uuid::new_v4();
        let password_hash = hash_password("password123").expect("hash should succeed");
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                &password_hash,
                "user",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                &password_hash,
                "user",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                &password_hash,
                "user",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-login-1",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .login("alice@example.com", "password123")
            .await
            .expect("login should succeed");

        assert_eq!(bundle.refresh_token, "refresh-login-1");
    }

    #[tokio::test]
    async fn login_admin_claim_contains_admin_and_user() {
        let user_id = Uuid::new_v4();
        let password_hash = hash_password("password123").expect("hash should succeed");
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"admin-claim-secret")
            .with_query_results([vec![user_model(
                user_id,
                "admin@example.com",
                &password_hash,
                "admin",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "admin@example.com",
                &password_hash,
                "admin",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "admin@example.com",
                &password_hash,
                "admin",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-login-admin",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .login("admin@example.com", "password123")
            .await
            .expect("login should succeed");
        let claims = provider
            .verify(&bundle.access_token)
            .await
            .expect("token should verify");

        assert_eq!(claims.roles, vec![Role::Admin, Role::User]);
    }

    #[tokio::test]
    async fn login_unknown_role_falls_back_to_user() {
        let user_id = Uuid::new_v4();
        let password_hash = hash_password("password123").expect("hash should succeed");
        let provider = ProviderFixtureBuilder::new()
            .with_secret(b"unknown-role-secret")
            .with_query_results([vec![user_model(
                user_id,
                "role@example.com",
                &password_hash,
                "manager",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "role@example.com",
                &password_hash,
                "manager",
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "role@example.com",
                &password_hash,
                "manager",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-login-unknown",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .login("role@example.com", "password123")
            .await
            .expect("login should succeed");
        let claims = provider
            .verify(&bundle.access_token)
            .await
            .expect("token should verify");

        assert_eq!(claims.roles, vec![Role::User]);
    }

    #[tokio::test]
    async fn refresh_rejects_missing_token() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<refresh_token::Model>::new()])
            .build();

        let err = provider
            .refresh("missing-token")
            .await
            .expect_err("refresh should fail");

        assert_eq!(err.message(), "Invalid refresh token");
    }

    #[tokio::test]
    async fn refresh_rejects_expired_token() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![refresh_token_model(
                "expired-token",
                user_id,
                Utc::now().fixed_offset() - Duration::minutes(1),
                false,
            )]])
            .build();

        let err = provider
            .refresh("expired-token")
            .await
            .expect_err("refresh should fail");

        assert_eq!(err.message(), "Refresh token expired");
    }

    #[tokio::test]
    async fn refresh_rejects_revoked_token() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![refresh_token_model(
                "revoked-token",
                user_id,
                Utc::now().fixed_offset() + Duration::days(1),
                true,
            )]])
            .build();

        let err = provider
            .refresh("revoked-token")
            .await
            .expect_err("refresh should fail");

        assert_eq!(err.message(), "Refresh token expired");
    }

    #[tokio::test]
    async fn refresh_rejects_missing_user_for_token() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![refresh_token_model(
                "valid-token",
                user_id,
                Utc::now().fixed_offset() + Duration::days(1),
                false,
            )]])
            .with_query_results([Vec::<user::Model>::new()])
            .build();

        let err = provider
            .refresh("valid-token")
            .await
            .expect_err("refresh should fail");

        assert_eq!(err.message(), "Invalid refresh token");
    }

    #[tokio::test]
    async fn refresh_returns_new_token_bundle_on_success() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![refresh_token_model(
                "old-refresh-token",
                user_id,
                Utc::now().fixed_offset() + Duration::days(1),
                false,
            )]])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                "hashed-password",
                "user",
            )]])
            .with_exec_result(1)
            .with_query_results([vec![refresh_token_model(
                "new-refresh-token",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .refresh("old-refresh-token")
            .await
            .expect("refresh should succeed");

        assert_eq!(bundle.refresh_token, "new-refresh-token");
    }

    #[tokio::test]
    async fn seed_admin_noops_when_admin_exists() {
        let admin_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([vec![user_model(
                admin_id,
                "admin@example.com",
                "hashed-password",
                "admin",
            )]])
            .build();

        let result = provider
            .seed_admin(&test_config("admin@example.com", "admin-password"))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn seed_admin_creates_admin_when_missing() {
        let admin_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .with_query_results([vec![user_model(
                admin_id,
                "admin@example.com",
                "hashed-password",
                "admin",
            )]])
            .build();

        let result = provider
            .seed_admin(&test_config("admin@example.com", "admin-password"))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn seed_admin_fails_when_admin_password_too_short() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .build();

        let err = provider
            .seed_admin(&test_config("admin@example.com", "short"))
            .await
            .expect_err("seed_admin should fail");

        assert!(err.to_string().starts_with("admin seed hash error:"));
    }

    #[tokio::test]
    async fn seed_admin_propagates_create_user_error() {
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .with_query_error(DbErr::Custom("insert failed".to_string()))
            .build();

        let err = provider
            .seed_admin(&test_config("admin@example.com", "admin-password"))
            .await
            .expect_err("seed_admin should fail");

        assert_eq!(
            err.to_string(),
            "database operation failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn token_bundle_uses_expected_type_and_ttl() {
        let user_id = Uuid::new_v4();
        let provider = ProviderFixtureBuilder::new()
            .with_query_results([Vec::<user::Model>::new()])
            .with_query_results([vec![user_model(
                user_id,
                "alice@example.com",
                "hashed-password",
                "user",
            )]])
            .with_query_results([vec![refresh_token_model(
                "refresh-register-ttl",
                user_id,
                Utc::now().fixed_offset() + Duration::days(30),
                false,
            )]])
            .build();

        let bundle = provider
            .register("alice@example.com", "password123")
            .await
            .expect("register should succeed");

        assert_eq!(bundle.token_type, "Bearer");
        assert_eq!(bundle.expires_in, ACCESS_TTL_SECS);
    }
}
