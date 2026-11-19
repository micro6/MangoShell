use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::State;

use crate::forward::ForwardManager;
use crate::record::RecordingManager;
use crate::secrets;
use crate::session::SessionManager;
use crate::sftp::SftpManager;
use crate::terminal::TerminalManager;

/// 全局应用状态，随 Builder::manage 注入
pub struct AppState {
    pub db: SqlitePool,
    pub db_path: PathBuf,
    pub sessions: SessionManager,
    pub terminals: TerminalManager,
    pub sftp: SftpManager,
    pub forwards: ForwardManager,
    pub recordings: RecordingManager,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHealth {
    pub version: String,
    pub db_path: String,
    pub schema_version: i64,
    pub host_count: i64,
}

/// 主机元数据：不含任何凭据
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HostRecord {
    pub id: String,
    pub alias: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub key_id: Option<String>,
}

/// 前端启动自检：确认 IPC 与数据库都可用
#[tauri::command]
pub async fn app_health(state: State<'_, AppState>) -> Result<AppHealth, String> {
    let schema_version: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
            .fetch_one(&state.db)
            .await
            .map_err(|e| e.to_string())?;

    let host_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM host")
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(AppHealth {
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: state.db_path.display().to_string(),
        schema_version,
        host_count,
    })
}

/// 主机列表：按最近更新排序
#[tauri::command]
pub async fn host_list(state: State<'_, AppState>) -> Result<Vec<HostRecord>, String> {
    sqlx::query_as::<_, HostRecord>(
        "SELECT id, alias, host, port, username, auth_type, key_id
         FROM host ORDER BY updated_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

/// 保存主机：id 为空则新增，否则更新；密码等凭据不落库
#[tauri::command]
pub async fn host_save(
    state: State<'_, AppState>,
    id: Option<String>,
    alias: String,
    host: String,
    port: u16,
    username: String,
    auth_type: Option<String>,
    key_id: Option<String>,
) -> Result<HostRecord, String> {
    let alias = alias.trim();
    let host = host.trim();
    let username = username.trim();

    if host.is_empty() || username.is_empty() {
        return Err("主机地址和用户名不能为空".to_string());
    }

    let auth_type = auth_type.unwrap_or_else(|| "password".to_string());
    if auth_type != "password" && auth_type != "key" {
        return Err("不支持的认证方式".to_string());
    }
    let key_id = key_id.filter(|k| !k.is_empty());
    if auth_type == "key" && key_id.is_none() {
        return Err("请选择登录密钥".to_string());
    }

    let alias = if alias.is_empty() { host } else { alias };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();

    let host_id = match id.filter(|v| !v.is_empty()) {
        Some(existing) => {
            let affected = sqlx::query(
                "UPDATE host
                 SET alias = ?1, host = ?2, port = ?3, username = ?4,
                     auth_type = ?5, key_id = ?6, updated_at = ?7
                 WHERE id = ?8",
            )
            .bind(alias)
            .bind(host)
            .bind(port as i64)
            .bind(username)
            .bind(&auth_type)
            .bind(&key_id)
            .bind(now)
            .bind(&existing)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
            if affected == 0 {
                return Err("主机不存在，可能已被删除".to_string());
            }
            existing
        }
        None => {
            let new_id = uuid::Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO host
                   (id, alias, host, port, username, auth_type, key_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            )
            .bind(&new_id)
            .bind(alias)
            .bind(host)
            .bind(port as i64)
            .bind(username)
            .bind(&auth_type)
            .bind(&key_id)
            .bind(now)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
            new_id
        }
    };

    Ok(HostRecord {
        id: host_id,
        alias: alias.to_string(),
        host: host.to_string(),
        port: port as i64,
        username: username.to_string(),
        auth_type,
        key_id,
    })
}

#[tauri::command]
pub async fn host_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // 主机删除时一并清理已保存的登录密码，失败只记录日志不阻塞删除
    let host_id = id.clone();
    let cleared =
        tauri::async_runtime::spawn_blocking(move || secrets::delete_host_password(&host_id)).await;
    let cleared = match cleared {
        Ok(result) => result,
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = cleared {
        log::warn!("删除主机 {id} 的已保存密码失败: {e}");
    }

    sqlx::query("DELETE FROM host WHERE id = ?1")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 记住密码：把登录密码写入系统密钥环
#[tauri::command]
pub async fn host_password_set(host_id: String, password: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || secrets::save_host_password(&host_id, &password))
        .await
        .map_err(|e| format!("写入系统密钥环失败: {e}"))?
}

/// 取消记住密码：删除系统密钥环中的条目
#[tauri::command]
pub async fn host_password_clear(host_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || secrets::delete_host_password(&host_id))
        .await
        .map_err(|e| format!("删除系统密钥环条目失败: {e}"))?
}

/// 查询主机是否已保存密码；不返回密码本身
#[tauri::command]
pub async fn host_password_exists(host_id: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || secrets::load_host_password(&host_id))
        .await
        .map_err(|e| format!("读取系统密钥环失败: {e}"))?
        .map(|password| password.is_some())
}

/// 已知主机列表：指纹由存储的 key_blob 现算，不额外落库
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownHostRecord {
    pub host: String,
    pub port: i64,
    pub key_type: String,
    pub fingerprint: String,
    pub added_at: i64,
}

#[derive(sqlx::FromRow)]
struct KnownHostRow {
    host: String,
    port: i64,
    key_type: String,
    key_blob: Vec<u8>,
    added_at: i64,
}

#[tauri::command]
pub async fn known_host_list(state: State<'_, AppState>) -> Result<Vec<KnownHostRecord>, String> {
    let rows = sqlx::query_as::<_, KnownHostRow>(
        "SELECT host, port, key_type, key_blob, added_at
         FROM known_host ORDER BY added_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .map(|row| KnownHostRecord {
            fingerprint: PublicKey::from_bytes(&row.key_blob)
                .map(|key| key.fingerprint(HashAlg::Sha256).to_string())
                .unwrap_or_else(|_| "无法解析".to_string()),
            host: row.host,
            port: row.port,
            key_type: row.key_type,
            added_at: row.added_at,
        })
        .collect())
}

#[tauri::command]
pub async fn known_host_delete(
    state: State<'_, AppState>,
    host: String,
    port: u16,
    key_type: String,
) -> Result<(), String> {
    sqlx::query("DELETE FROM known_host WHERE host = ?1 AND port = ?2 AND key_type = ?3")
        .bind(&host)
        .bind(port as i64)
        .bind(&key_type)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
