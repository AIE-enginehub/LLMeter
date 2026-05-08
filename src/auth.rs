use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;

// ============================================================
// JWT Claims
// ============================================================

/// JWT 载荷
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// 管理员用户 ID
    pub sub: String,
    /// 过期时间 (Unix 时间戳)
    pub exp: usize,
}

/// 生成 JWT token（有效期 24 小时）
pub fn generate_token(secret: &str, user_id: &str) -> String {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::TimeDelta::hours(24))
        .expect("valid timestamp")
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("JWT encoding should not fail")
}

/// 验证 JWT token 并返回 Claims
pub fn verify_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

// ============================================================
// 密码处理 (Argon2)
// ============================================================

/// 使用 Argon2 对密码进行哈希
pub fn hash_password(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("password hashing should not fail")
        .to_string()
}

/// 验证密码是否与哈希匹配
pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

// ============================================================
// Axum 认证提取器
// ============================================================

/// 从请求中提取并验证管理员身份的 Axum extractor。
/// 优先从 Cookie 中的 `token` 字段提取 JWT，回退到 Authorization Bearer header。
pub struct AuthAdmin {
    pub user_id: String,
}

impl FromRequestParts<Arc<AppState>> for AuthAdmin {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token_from_cookie(parts)
            .or_else(|| extract_token_from_header(parts))
            .ok_or((StatusCode::UNAUTHORIZED, "Missing authentication token"))?;

        let claims = verify_token(&state.jwt_secret, &token)
            .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(AuthAdmin {
            user_id: claims.sub,
        })
    }
}

/// 从 Cookie header 中提取名为 `token` 的值
fn extract_token_from_cookie(parts: &Parts) -> Option<String> {
    let cookie_header = parts.headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header
        .split(';')
        .map(|s| s.trim())
        .find(|s| s.starts_with("token="))
        .map(|s| s.trim_start_matches("token=").to_string())
}

/// 从 Authorization: Bearer <token> header 中提取 token
fn extract_token_from_header(parts: &Parts) -> Option<String> {
    let auth = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ").map(|t| t.to_string())
}
