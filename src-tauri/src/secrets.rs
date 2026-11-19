//! 私钥正文的 OS Keyring 存储。
//!
//! SQLite 只保留密钥元数据（`ssh_key.secret_ref` 指向 Keyring 条目），
//! 私钥本体写入 Windows 凭据管理器 / macOS Keychain / Linux Secret Service。
//! 旧版本把私钥正文存在 `ssh_key.private_key`，启动时会一次性迁移。
//!
//! Windows 凭据管理器单条正文上限 2560 字节，RSA 4096 的 PEM 会超限，
//! 因此按固定大小分块：第 0 块带块数头部，其余块只存正文。

use keyring::{Entry, Error};
use sqlx::SqlitePool;

/// Keyring 中的服务名，凭据管理器里显示为 MangoShell
const SERVICE: &str = "MangoShell";
/// 旧版本的 secret_ref 前缀：正文直接存在 SQLite
const LEGACY_REF_PREFIX: &str = "db:";
/// 每块正文上限，留出头部与平台编码的余量（Windows 上限 2560）
const CHUNK_BYTES: usize = 2000;
/// 分块格式头，用于识别块数
const CHUNK_HEADER: &str = "MGS1";

/// 新密钥的 secret_ref，标记正文位于系统密钥环
pub fn secret_ref(id: &str) -> String {
    format!("keyring:{id}")
}

/// 是否为旧版本遗留的 SQLite 存储引用
pub fn is_legacy_ref(secret_ref: &str) -> bool {
    secret_ref.starts_with(LEGACY_REF_PREFIX)
}

fn entry(id: &str, index: usize) -> Result<Entry, String> {
    let account = if index == 0 {
        format!("ssh-key:{id}")
    } else {
        format!("ssh-key:{id}#{index}")
    };
    Entry::new(SERVICE, &account).map_err(|e| describe(&e))
}

fn describe(err: &Error) -> String {
    match err {
        Error::NoDefaultStore => "系统密钥环不可用，请确认系统凭据服务已启用".to_string(),
        Error::NoStorageAccess(e) => format!("无法访问系统密钥环: {e}"),
        Error::PlatformFailure(e) => format!("系统密钥环操作失败: {e}"),
        Error::TooLong(field, limit) => {
            format!("私钥超出系统密钥环单条容量上限（{field} 上限 {limit} 字节）")
        }
        other => format!("系统密钥环操作失败: {other}"),
    }
}

/// 把正文切成适合单条凭据的块，首块带块数头
fn encode_chunks(pem: &str) -> Vec<Vec<u8>> {
    let body = pem.as_bytes();
    let total = body.len().div_ceil(CHUNK_BYTES).max(1);
    let mut chunks = Vec::with_capacity(total);
    for (index, part) in body.chunks(CHUNK_BYTES).enumerate() {
        if index == 0 {
            let mut chunk = format!("{CHUNK_HEADER} {total}\n").into_bytes();
            chunk.extend_from_slice(part);
            chunks.push(chunk);
        } else {
            chunks.push(part.to_vec());
        }
    }
    if chunks.is_empty() {
        chunks.push(format!("{CHUNK_HEADER} 1\n").into_bytes());
    }
    chunks
}

/// 解析首块头部，返回（块数，首块正文偏移）
fn parse_header(chunk: &[u8]) -> Option<(usize, usize)> {
    let newline = chunk.iter().position(|b| *b == b'\n')?;
    let header = std::str::from_utf8(&chunk[..newline]).ok()?;
    let total: usize = header.strip_prefix(CHUNK_HEADER)?.trim().parse().ok()?;
    (total > 0).then_some((total, newline + 1))
}

fn join_chunks(chunks: &[Vec<u8>]) -> Result<String, String> {
    let (_, offset) = parse_header(&chunks[0]).unwrap_or((1, 0));
    let mut bytes = chunks[0][offset..].to_vec();
    for chunk in &chunks[1..] {
        bytes.extend_from_slice(chunk);
    }
    String::from_utf8(bytes)
        .map_err(|_| "系统密钥环中的私钥数据已损坏，请重新导入该密钥".to_string())
}

/// 写入私钥正文；同 id 重复写入会覆盖旧条目
pub fn save(id: &str, pem: &str) -> Result<(), String> {
    let chunks = encode_chunks(pem);
    // 先写后续块，最后写首块作为提交点，避免读到半截数据
    for index in (1..chunks.len()).rev() {
        if let Err(e) = entry(id, index)?.set_secret(&chunks[index]) {
            // 清理已写入的块，避免残留
            for leftover in 0..chunks.len() {
                if let Ok(item) = entry(id, leftover) {
                    let _ = item.delete_credential();
                }
            }
            return Err(describe(&e));
        }
    }
    entry(id, 0)?
        .set_secret(&chunks[0])
        .map_err(|e| describe(&e))
}

/// 读取私钥正文；条目不存在时返回 None
pub fn load(id: &str) -> Result<Option<String>, String> {
    let first = match entry(id, 0)?.get_secret() {
        Ok(bytes) => bytes,
        Err(Error::NoEntry) => return Ok(None),
        Err(e) => return Err(describe(&e)),
    };

    let Some((total, _)) = parse_header(&first) else {
        // 兼容未分块的历史单条格式
        return join_chunks(&[first]).map(Some);
    };

    let mut chunks = vec![first];
    for index in 1..total {
        match entry(id, index)?.get_secret() {
            Ok(bytes) => chunks.push(bytes),
            Err(Error::NoEntry) => {
                return Err("系统密钥环中的私钥数据不完整，请重新导入该密钥".to_string())
            }
            Err(e) => return Err(describe(&e)),
        }
    }
    join_chunks(&chunks).map(Some)
}

/// 删除私钥正文；条目本就不存在时视为成功
pub fn delete(id: &str) -> Result<(), String> {
    let total = match entry(id, 0)?.get_secret() {
        Ok(bytes) => parse_header(&bytes).map(|(total, _)| total).unwrap_or(1),
        Err(Error::NoEntry) => return Ok(()),
        Err(e) => return Err(describe(&e)),
    };

    for index in 0..total {
        match entry(id, index)?.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => {}
            Err(e) => return Err(describe(&e)),
        }
    }
    Ok(())
}

/// 启动时把旧版本落在 SQLite 的私钥正文迁移到系统密钥环。
///
/// 单条迁移失败只记录日志并保留原值，不阻塞应用启动；下次启动会重试。
pub async fn migrate_legacy(pool: &SqlitePool) {
    let rows: Vec<(String, String)> =
        match sqlx::query_as("SELECT id, private_key FROM ssh_key WHERE private_key <> ''")
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("检查待迁移密钥失败: {e}");
                return;
            }
        };

    for (id, pem) in rows {
        let task = {
            let id = id.clone();
            let pem = pem.clone();
            tauri::async_runtime::spawn_blocking(move || save(&id, &pem))
        };
        if !matches!(task.await, Ok(Ok(()))) {
            log::warn!("密钥 {id} 迁移到系统密钥环失败，暂保留 SQLite 中的正文");
            continue;
        }

        let updated =
            sqlx::query("UPDATE ssh_key SET secret_ref = ?1, private_key = '' WHERE id = ?2")
                .bind(secret_ref(&id))
                .bind(&id)
                .execute(pool)
                .await;
        match updated {
            Ok(_) => log::info!("密钥 {id} 已迁移到系统密钥环"),
            Err(e) => log::warn!("密钥 {id} 迁移后更新元数据失败: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CHUNK_BYTES, encode_chunks, is_legacy_ref, join_chunks, parse_header, secret_ref};

    #[test]
    fn distinguishes_legacy_and_keyring_refs() {
        assert!(is_legacy_ref("db:00000000-0000-0000-0000-000000000000"));
        assert!(!is_legacy_ref(&secret_ref("abc")));
        assert_eq!(secret_ref("abc"), "keyring:abc");
    }

    #[test]
    fn splits_large_pem_into_bounded_chunks() {
        let pem = "k".repeat(5000);
        let chunks = encode_chunks(&pem);

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 2560));
        let (total, offset) = parse_header(&chunks[0]).expect("首块应带头部");
        assert_eq!(total, chunks.len());
        assert_eq!(join_chunks(&chunks).unwrap(), pem);
        assert_eq!(chunks[0].len() - offset, CHUNK_BYTES);
    }

    #[test]
    fn keeps_small_pem_in_single_chunk() {
        let pem = "small-key";
        let chunks = encode_chunks(pem);

        assert_eq!(chunks.len(), 1);
        assert_eq!(join_chunks(&chunks).unwrap(), pem);
    }
}
