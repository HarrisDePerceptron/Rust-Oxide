use async_trait::async_trait;

use crate::{
    auth::{
        Claims, Role, TokenBundle,
        jwt::{JwtKeys, encode_token, make_access_claims},
        password::{hash_password, verify_password},
    },
    config::AppConfig,
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

    async fn seed_admin(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        if let Some(existing) = self
            .user_service
            .find_by_email(&cfg.admin_email)
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?
        {
            tracing::info!("admin user already present: {}", existing.email);
            return Ok(());
        }

        let hash = hash_password(&cfg.admin_password)
            .map_err(|e| anyhow::anyhow!("admin seed hash error: {}", e.to_string()))?;
        let user = self
            .user_service
            .create_user(&cfg.admin_email, &hash, Role::Admin.as_str())
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        tracing::info!("seeded admin user {}", user.email);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm::{DatabaseBackend, MockDatabase};
    use uuid::Uuid;

    use crate::{
        auth::{
            Role,
            jwt::{encode_token, make_access_claims},
            providers::AuthProvider,
        },
        services::ServiceContext,
    };

    use super::{AuthProviderId, LocalAuthProvider};

    fn test_provider(secret: &[u8]) -> LocalAuthProvider {
        let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
        let services = ServiceContext::new(&db);
        LocalAuthProvider::new(
            services.user(),
            services.refresh_token_dao(),
            crate::auth::jwt::JwtKeys::from_secret(secret),
        )
    }

    #[tokio::test]
    async fn provider_id_is_local() {
        let provider = test_provider(b"provider-id-secret");
        assert_eq!(provider.id(), AuthProviderId::Local);
    }

    #[tokio::test]
    async fn verify_accepts_valid_token() {
        let provider = test_provider(b"verify-secret");
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
        assert_eq!(verified.roles, claims.roles);
    }

    #[tokio::test]
    async fn verify_rejects_invalid_token() {
        let provider = test_provider(b"verify-secret");
        let err = provider
            .verify("not-a-jwt")
            .await
            .expect_err("verify should fail");
        assert!(
            err.message().starts_with("Invalid or expired token:"),
            "unexpected message: {}",
            err.message()
        );
    }

    #[tokio::test]
    async fn verify_rejects_token_signed_with_different_secret() {
        let provider = test_provider(b"provider-secret-a");
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
        assert!(
            err.message().starts_with("Invalid or expired token:"),
            "unexpected message: {}",
            err.message()
        );
    }
}
