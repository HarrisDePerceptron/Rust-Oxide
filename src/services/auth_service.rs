use axum::http::StatusCode;

use crate::{
    auth::{
        Role,
        jwt::{encode_token, make_access_claims},
        password::{hash_password, verify_password},
    },
    config::AppConfig,
    db::dao::{DaoBase, DaoLayerError, RefreshTokenDao, UserDao},
    db::entities,
    error::AppError,
    state::JwtKeys,
};

const ACCESS_TTL_SECS: usize = 15 * 60; // 15 minutes
const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Debug)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: usize,
}

#[derive(Clone)]
pub struct AuthService {
    user_dao: UserDao,
    refresh_token_dao: RefreshTokenDao,
    jwt: JwtKeys,
}

impl AuthService {
    pub fn new(user_dao: UserDao, refresh_token_dao: RefreshTokenDao, jwt: JwtKeys) -> Self {
        Self {
            user_dao,
            refresh_token_dao,
            jwt,
        }
    }

    pub async fn register(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        let email = email.trim();
        if email.is_empty() {
            return Err(AppError::new(StatusCode::BAD_REQUEST, "Email required"));
        }

        if self
            .user_dao
            .find_by_email(email)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
            .is_some()
        {
            return Err(AppError::new(StatusCode::CONFLICT, "User already exists"));
        }

        let password_hash = hash_password(password)?;
        let user = self
            .user_dao
            .create_user(email, &password_hash, Role::User.as_str())
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Create user failed"))?;

        self.issue_tokens(&user).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<TokenBundle, AppError> {
        let user = self
            .user_dao
            .find_by_email(email)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
            .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid credentials"))?;

        let password_ok = verify_password(password, &user.password_hash)?;
        if !password_ok {
            return Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Invalid credentials",
            ));
        }

        let now = chrono::Utc::now().fixed_offset();
        self.user_dao
            .set_last_login(&user.id, &now)
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to update last login",
                )
            })?;

        self.issue_tokens(&user).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenBundle, AppError> {
        let token = self
            .refresh_token_dao
            .find_active_by_token(refresh_token)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"))?
            .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "Invalid refresh token"))?;

        if token.expires_at < chrono::Utc::now().fixed_offset() || token.revoked {
            return Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "Refresh token expired",
            ));
        }

        let user = self
            .user_dao
            .find_by_id(token.user_id)
            .await
            .map_err(|err| match err {
                DaoLayerError::NotFound { .. } => {
                    AppError::new(StatusCode::UNAUTHORIZED, "User missing")
                }
                _ => AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "DB error"),
            })?;

        self.refresh_token_dao
            .revoke_token(refresh_token)
            .await
            .map_err(|_| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "Token revoke failed"))?;

        self.issue_tokens(&user).await
    }

    pub async fn seed_admin(&self, cfg: &AppConfig) -> anyhow::Result<()> {
        if let Some(existing) = self
            .user_dao
            .find_by_email(&cfg.admin_email)
            .await
            .map_err(map_dao_error)?
        {
            tracing::info!("admin user already present: {}", existing.email);
            return Ok(());
        }

        let hash = hash_password(&cfg.admin_password)
            .map_err(|e| anyhow::anyhow!("admin seed hash error: {}", e.message))?;
        let user = self
            .user_dao
            .create_user(&cfg.admin_email, &hash, Role::Admin.as_str())
            .await
            .map_err(map_dao_error)?;
        tracing::info!("seeded admin user {}", user.email);
        Ok(())
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
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Refresh token issue failed",
                )
            })?;

        Ok(TokenBundle {
            access_token,
            refresh_token: refresh.token,
            token_type: "Bearer",
            expires_in: ACCESS_TTL_SECS,
        })
    }
}

fn map_dao_error(err: DaoLayerError) -> anyhow::Error {
    match err {
        DaoLayerError::Db(db_err) => anyhow::anyhow!(db_err),
        DaoLayerError::NotFound { entity, .. } => anyhow::anyhow!("{entity} not found"),
        DaoLayerError::InvalidPagination { page, page_size } => anyhow::anyhow!(
            "Invalid pagination: page={page} page_size={page_size}"
        ),
    }
}
