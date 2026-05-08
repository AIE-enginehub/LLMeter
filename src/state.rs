use sqlx::PgPool;

/// 全局共享的应用状态
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub http_client: reqwest::Client,
    pub jwt_secret: String,
}
