use russh::keys::ssh_key::LineEnding;
use russh::keys::{HashAlg, PrivateKey};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

/// 密钥元数据：只返回元数据与公钥，私钥正文绝不出现在 IPC 返回值里
#[derive(Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct KeyRecord {
    pub id: String,
    pub name: String,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint: String,
    pub created_at: i64,
}

#[tauri::command]
pub async fn key_list(state: State<'_, AppState>) -> Result<Vec<KeyRecord>, String> {
    sqlx::query_as::<_, KeyRecord>(
        "SELECT id, name, key_type, public_key, fingerprint, created_at
         FROM ssh_key ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

/// 导入 OpenSSH 私钥：加密私钥当场解密，口令本身不保存
#[tauri::command]
pub async fn key_import(
    state: State<'_, AppState>,
    name: String,
    path: String,
    passphrase: Option<String>,
) -> Result<KeyRecord, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取私钥文件失败: {e}"))?;
    let mut key = PrivateKey::from_openssh(&text)
        .map_err(|_| "私钥解析失败，仅支持 OpenSSH 格式（如 id_ed25519 / id_rsa）".to_string())?;

    if key.is_encrypted() {
        let passphrase = passphrase
            .filter(|p| !p.is_empty())
            .ok_or_else(|| "该私钥已加密，请输入口令".to_string())?;
        key = key
            .decrypt(passphrase.as_bytes())
            .map_err(|_| "私钥口令不正确".to_string())?;
    }

    let public_key = key
        .public_key()
        .to_openssh()
        .map_err(|e| format!("导出公钥失败: {e}"))?;
    let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
    let key_type = key.algorithm().as_str().to_string();
    // 统一存已解密的 PEM，避免连接时再要求输入口令
    let private_pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| format!("私钥序列化失败: {e}"))?;

    let exists: Option<String> = sqlx::query_scalar("SELECT id FROM ssh_key WHERE fingerprint = ?1")
        .bind(&fingerprint)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if exists.is_some() {
        return Err("该密钥已导入".to_string());
    }

    let id = uuid::Uuid::new_v4().to_string();
    let trimmed = name.trim();
    let name = if trimmed.is_empty() {
        key_type.clone()
    } else {
        trimmed.to_string()
    };
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();

    sqlx::query(
        "INSERT INTO ssh_key
           (id, name, key_type, bits, public_key, fingerprint, secret_ref, private_key, created_at)
         VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&key_type)
    .bind(&public_key)
    .bind(&fingerprint)
    .bind(format!("db:{id}"))
    .bind(&*private_pem)
    .bind(created_at)
    .execute(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    log::info!("已导入 SSH 密钥: {name} ({key_type}) {fingerprint}");
    Ok(KeyRecord {
        id,
        name,
        key_type,
        public_key,
        fingerprint,
        created_at,
    })
}

#[tauri::command]
pub async fn key_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let used: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM host WHERE key_id = ?1")
        .bind(&id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if used > 0 {
        return Err(format!("该密钥仍被 {used} 台主机使用，请先修改主机认证方式"));
    }

    sqlx::query("DELETE FROM ssh_key WHERE id = ?1")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
