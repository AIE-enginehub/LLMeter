use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{self, AuthAdmin};
use crate::state::AppState;
use super::{err, AdminUserRow, ErrorResponse};

#[derive(Deserialize)]
pub(super) struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    user: UserInfo,
}

#[derive(Serialize)]
pub(super) struct UserInfo {
    id: Uuid,
    username: String,
}

/// POST /api/auth/login — 管理员登录
pub(super) async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Invalid username or password"))?;

    if !auth::verify_password(&body.password, &user.password_hash) {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password",
        ));
    }

    let token = auth::generate_token(&state.jwt_secret, &user.id.to_string());

    let cookie = format!(
        "token={token}; HttpOnly; Path=/; Max-Age=86400; SameSite=Lax"
    );

    let response = LoginResponse {
        token: token.clone(),
        user: UserInfo {
            id: user.id,
            username: user.username,
        },
    };

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(response),
    ))
}

/// POST /api/auth/logout — 登出，清除 cookie
pub(super) async fn logout(
    _admin: AuthAdmin,
) -> impl IntoResponse {
    let cookie = "token=; HttpOnly; Path=/; Max-Age=0; SameSite=Lax";
    (StatusCode::OK, [(header::SET_COOKIE, cookie.to_string())], Json(serde_json::json!({"ok": true})))
}

/// GET /api/auth/me — 获取当前登录用户信息
pub(super) async fn me(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
) -> Result<Json<UserInfo>, (StatusCode, Json<ErrorResponse>)> {
    let user_id: Uuid = admin
        .user_id
        .parse()
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid user id in token"))?;

    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "User not found"))?;

    Ok(Json(UserInfo {
        id: user.id,
        username: user.username,
    }))
}

#[derive(Deserialize)]
pub(super) struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

/// PUT /api/auth/password — 修改当前管理员密码
pub(super) async fn change_password(
    State(state): State<Arc<AppState>>,
    admin: AuthAdmin,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if body.new_password.len() < 6 {
        return Err(err(StatusCode::BAD_REQUEST, "新密码长度不能少于 6 位"));
    }

    let user_id: Uuid = admin
        .user_id
        .parse()
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid user id in token"))?;

    let user = sqlx::query_as::<_, AdminUserRow>(
        "SELECT id, username, password_hash, created_at FROM admin_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !auth::verify_password(&body.old_password, &user.password_hash) {
        return Err(err(StatusCode::BAD_REQUEST, "原密码错误"));
    }

    let new_hash = auth::hash_password(&body.new_password);
    sqlx::query("UPDATE admin_users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&state.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({"ok": true})))
}
