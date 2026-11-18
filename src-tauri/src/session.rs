use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use russh::client::{self, Handle};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const KEEPALIVE_MAX: usize = 3;

/// 一台主机对应一条 TCP 连接：终端 / SFTP / 转发后续都复用这条 SSH 会话
pub struct HostConnection {
    pub key: String,
    pub handle: Handle<HostKeyVerifier>,
}

/// 会话池：user@host:port → 连接，命中即复用
pub struct SessionManager {
    pool: SqlitePool,
    conns: RwLock<HashMap<String, Arc<HostConnection>>>,
    /// host_id → 密码。仅保存在内存，M2 迁移到 OS Keyring
    passwords: RwLock<HashMap<String, String>>,
}

#[derive(sqlx::FromRow)]
struct HostRow {
    host: String,
    port: i64,
    username: String,
    auth_type: String,
    key_id: Option<String>,
}

impl SessionManager {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            conns: RwLock::new(HashMap::new()),
            passwords: RwLock::new(HashMap::new()),
        }
    }

    /// 取连接：命中则复用；未命中则用传入密码（或本进程内已记住的密码）建连并认证
    pub async fn acquire(
        &self,
        host_id: &str,
        password: Option<&str>,
    ) -> anyhow::Result<Arc<HostConnection>> {
        let row: HostRow = sqlx::query_as(
            "SELECT host, port, username, auth_type, key_id FROM host WHERE id = ?1",
        )
        .bind(host_id)
        .fetch_optional(&self.pool)
        .await
        .context("读取主机记录失败")?
        .ok_or_else(|| anyhow!("主机不存在，请重新创建连接"))?;

        let key = format!("{}@{}:{}", row.username, row.host, row.port);
        {
            let conns = self.conns.read().await;
            if let Some(conn) = conns.get(&key) {
                // 池中连接可能已被远端关闭，先探活再复用
                if !conn.handle.is_closed() {
                    return Ok(conn.clone());
                }
            }
        }
        if self.conns.write().await.remove(&key).is_some() {
            log::warn!("丢弃已断开的 SSH 连接: {key}");
        }

        let handle = if row.auth_type == "key" {
            self.connect_by_key(&row).await?
        } else {
            let password = match password {
                Some(p) if !p.is_empty() => p.to_string(),
                _ => self
                    .passwords
                    .read()
                    .await
                    .get(host_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("请输入登录密码"))?,
            };
            let handle = self
                .connect(&row.host, row.port as u16, &row.username, &password)
                .await?;
            self.passwords
                .write()
                .await
                .insert(host_id.to_string(), password);
            handle
        };

        let conn = Arc::new(HostConnection {
            key: key.clone(),
            handle,
        });
        self.conns.write().await.insert(key, conn.clone());
        log::info!("SSH 连接已建立: {}", conn.key);
        Ok(conn)
    }

    async fn connect(
        &self,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
    ) -> anyhow::Result<Handle<HostKeyVerifier>> {
        let config = client_config();

        let verifier = HostKeyVerifier::new(self.pool.clone(), host.to_string(), port);
        log::info!("正在连接 {username}@{host}:{port}");
        let mut handle = client::connect(config, (host, port), verifier)
            .await
            .with_context(|| format!("无法连接到 {host}:{port}"))?;

        let result = handle
            .authenticate_password(username, password)
            .await
            .context("发送认证请求失败")?;
        if !result.success() {
            log::warn!("认证被拒绝 {username}@{host}:{port}: {result:?}");
            bail!("认证失败：用户名或密码错误");
        }
        Ok(handle)
    }

    /// 使用主机绑定的私钥认证；私钥正文在连接时从本机库读取
    async fn connect_by_key(&self, row: &HostRow) -> anyhow::Result<Handle<HostKeyVerifier>> {
        let key_id = row
            .key_id
            .as_deref()
            .ok_or_else(|| anyhow!("该主机使用密钥认证，但尚未选择密钥"))?;

        let pem: Option<String> =
            sqlx::query_scalar("SELECT private_key FROM ssh_key WHERE id = ?1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await
                .context("读取登录密钥失败")?;
        let pem = pem
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("登录密钥不存在，请重新导入"))?;

        let key = PrivateKey::from_openssh(pem.as_bytes()).context("解析登录密钥失败")?;
        self.connect_with_key(&row.host, row.port as u16, &row.username, key)
            .await
    }

    async fn connect_with_key(
        &self,
        host: &str,
        port: u16,
        username: &str,
        key: PrivateKey,
    ) -> anyhow::Result<Handle<HostKeyVerifier>> {
        let verifier = HostKeyVerifier::new(self.pool.clone(), host.to_string(), port);
        log::info!("正在连接 {username}@{host}:{port}（密钥认证）");
        let mut handle = client::connect(client_config(), (host, port), verifier)
            .await
            .with_context(|| format!("无法连接到 {host}:{port}"))?;

        // RSA 需要与服务端协商 hash 算法，其他类型该参数会被忽略
        let hash_alg = if key.algorithm().is_rsa() {
            handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten()
        } else {
            None
        };

        let result = handle
            .authenticate_publickey(
                username,
                PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
            )
            .await
            .context("发送认证请求失败")?;
        if !result.success() {
            log::warn!("密钥认证被拒绝 {username}@{host}:{port}: {result:?}");
            bail!("密钥认证失败：请确认公钥已加入远端 authorized_keys");
        }
        Ok(handle)
    }
}

fn client_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        nodelay: true,
        ..Default::default()
    })
}

/// 主机密钥校验器：已信任的必须完全一致，首次连接按 TOFU 记录
pub struct HostKeyVerifier {
    pool: SqlitePool,
    host: String,
    port: u16,
}

impl HostKeyVerifier {
    pub fn new(pool: SqlitePool, host: String, port: u16) -> Self {
        Self { pool, host, port }
    }
}

impl client::Handler for HostKeyVerifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let public_key = server_public_key.public_key();
        let blob = match public_key.to_bytes() {
            Ok(blob) => blob,
            Err(e) => {
                log::warn!("主机密钥编码失败，拒绝连接: {e}");
                return Ok(false);
            }
        };
        let key_type = public_key.algorithm().as_str().to_string();
        let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();

        let trusted: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT key_blob FROM known_host WHERE host = ?1 AND port = ?2 AND key_type = ?3",
        )
        .bind(&self.host)
        .bind(self.port as i64)
        .bind(&key_type)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            // 读不到既有记录就无法判断是否被替换，fail-closed
            log::error!("读取 known_host 失败: {e}");
            russh::Error::UnknownKey
        })?;

        match trusted {
            Some(existing) => {
                if existing == blob {
                    Ok(true)
                } else {
                    log::warn!(
                        "主机密钥不匹配，已拒绝连接: {}:{} {} {}",
                        self.host,
                        self.port,
                        key_type,
                        fingerprint
                    );
                    Ok(false)
                }
            }
            None => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or_default();
                if let Err(e) = sqlx::query(
                    "INSERT INTO known_host (host, port, key_type, key_blob, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(host, port, key_type) DO NOTHING",
                )
                .bind(&self.host)
                .bind(self.port as i64)
                .bind(&key_type)
                .bind(&blob)
                .bind(now)
                .execute(&self.pool)
                .await
                {
                    log::error!("记录 known_host 失败: {e}");
                    return Err(russh::Error::UnknownKey);
                }
                log::info!(
                    "首次连接，已信任主机密钥: {}:{} {} {}",
                    self.host,
                    self.port,
                    key_type,
                    fingerprint
                );
                Ok(true)
            }
        }
    }
}
