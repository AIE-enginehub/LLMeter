mod admin;
mod auth;
mod db;
mod open_api;
mod protocol;
mod proxy;
mod state;
mod static_files;

use state::AppState;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = std::env::var("AUTH_SECRET").expect("AUTH_SECRET must be set");
    let admin_password = std::env::var("ADMIN_INITIAL_PASSWORD").unwrap_or("admin123".to_string());

    let pool = db::init_pool(&database_url).await;
    db::run_migrations(&pool).await;
    ensure_admin_user(&pool, &admin_password).await;

    let state = Arc::new(AppState {
        pool,
        http_client: reqwest::Client::new(),
        jwt_secret,
    });

    // 管理后台 API + 代理/静态文件 fallback
    let app = admin::router()
        .merge(open_api::router())
        .fallback(proxy::proxy_handler)
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or("5000".to_string());
    let addr = format!("0.0.0.0:{port}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap_or_else(|e| {
        tracing::error!("端口 {port} 绑定失败: {e}（可能已被其他进程占用）");
        std::process::exit(1);
    });
    tracing::info!("gongs-credit listening on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");
}

/// 等待 Ctrl+C 信号以优雅关闭
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    tracing::info!("收到关闭信号，正在停止服务...");
}

/// 确保默认管理员用户存在
async fn ensure_admin_user(pool: &sqlx::PgPool, password: &str) {
    let exists: Option<bool> =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admin_users WHERE username = 'admin')")
            .fetch_one(pool)
            .await
            .ok();

    if exists != Some(true) {
        let hash = auth::hash_password(password);
        let _ = sqlx::query("INSERT INTO admin_users (username, password_hash) VALUES ('admin', $1)")
            .bind(&hash)
            .execute(pool)
            .await;
        tracing::info!("Default admin user created (username: admin)");
    }
}
