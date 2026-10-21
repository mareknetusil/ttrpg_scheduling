use crate::error::AppError;
use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::request::Parts,
};
use axum_extra::extract::cookie::CookieJar;
use std::sync::Arc;

#[derive(Debug, serde::Serialize)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
}

impl FromRequestParts<Arc<crate::AppState>> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<crate::AppState>,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar.get("session").ok_or(AppError::Unauthorized)?.value();
        sqlx::query_as!(
            CurrentUser,
            r#"SELECT u.id, u.username, u.display_name, u.is_admin AS "is_admin: bool"
            FROM auth_sessions s JOIN users u ON u.id = s.user_id
            WHERE s.id = ? AND s.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
              AND u.deactivated_at IS NULL"#,
            token
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)
    }
}

impl OptionalFromRequestParts<Arc<crate::AppState>> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<crate::AppState>,
    ) -> Result<Option<Self>, Self::Rejection> {
        match <CurrentUser as FromRequestParts<Arc<crate::AppState>>>::from_request_parts(
            parts, state,
        )
        .await
        {
            Ok(user) => Ok(Some(user)),
            Err(AppError::Unauthorized) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
