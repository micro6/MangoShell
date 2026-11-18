use std::path::PathBuf;

use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};

/// 初始化数据库：建目录、建连接池、执行内置迁移
pub async fn init(app: &AppHandle) -> anyhow::Result<(SqlitePool, PathBuf)> {
    let dir = app.path().app_data_dir().context("无法获取应用数据目录")?;
    std::fs::create_dir_all(&dir).context("创建应用数据目录失败")?;

    let db_path = dir.join("mangoshell.db");

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("连接 SQLite 失败")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("执行数据库迁移失败")?;

    log::info!("数据库就绪: {}", db_path.display());
    Ok((pool, db_path))
}
