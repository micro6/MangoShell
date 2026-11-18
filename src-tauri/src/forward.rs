use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tokio::task::{JoinHandle, JoinSet};

use crate::commands::AppState;
use crate::session::HostConnection;

/// 双向拷贝缓冲与统计推送间隔
const COPY_BUFFER: usize = 64 * 1024;
const STATS_INTERVAL: Duration = Duration::from_secs(1);

/// SOCKS5 建连成功应答：05 00 00 01 0.0.0.0:0
const SOCKS5_OK: [u8; 10] = [0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];

/// 单条规则的累计流量
#[derive(Default)]
pub struct Counters {
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
}

/// -R 回推通道用到的本地目标
pub struct ForwardTarget {
    pub host: String,
    pub port: u16,
    pub counters: Arc<Counters>,
}

/// 运行中的远程转发注册表：服务器回推通道时按监听端口查找本地目标
#[derive(Default)]
pub struct ForwardRegistry {
    rules: RwLock<HashMap<u16, Arc<ForwardTarget>>>,
}

impl ForwardRegistry {
    async fn insert(&self, port: u16, target: Arc<ForwardTarget>) {
        self.rules.write().await.insert(port, target);
    }

    async fn remove(&self, port: u16) {
        self.rules.write().await.remove(&port);
    }

    pub async fn lookup(&self, port: u16) -> Option<Arc<ForwardTarget>> {
        self.rules.read().await.get(&port).cloned()
    }
}

/// 运行中规则的句柄：停止时 abort 监听任务，任务析构会一并关闭监听与活跃通道
struct RunningForward {
    kind: String,
    bind_host: String,
    bound_port: u16,
    conn: Arc<HostConnection>,
    counters: Arc<Counters>,
    tasks: Vec<JoinHandle<()>>,
    /// 监听循环异常退出时写入的原因
    error: Arc<StdMutex<Option<String>>>,
}

pub struct ForwardManager {
    pub registry: Arc<ForwardRegistry>,
    running: Mutex<HashMap<String, RunningForward>>,
}

impl ForwardManager {
    pub fn new(registry: Arc<ForwardRegistry>) -> Self {
        Self {
            registry,
            running: Mutex::new(HashMap::new()),
        }
    }

    async fn take(&self, id: &str) -> Option<RunningForward> {
        self.running.lock().await.remove(id)
    }

    async fn put(&self, id: String, run: RunningForward) {
        self.running.lock().await.insert(id, run);
    }

    /// 运行状态快照：(bytes_in, bytes_out, status, error)
    async fn info(&self, id: &str) -> Option<(u64, u64, String, Option<String>)> {
        let running = self.running.lock().await;
        let run = running.get(id)?;
        let error = run
            .error
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .or_else(|| {
                run.conn
                    .handle
                    .is_closed()
                    .then(|| "SSH 连接已断开，请重新启动规则".to_string())
            });
        let status = if error.is_some() { "error" } else { "running" };
        Some((
            run.counters.bytes_in.load(Ordering::Relaxed),
            run.counters.bytes_out.load(Ordering::Relaxed),
            status.to_string(),
            error,
        ))
    }

    async fn snapshot(&self) -> Vec<ForwardStat> {
        let running = self.running.lock().await;
        running
            .iter()
            .map(|(id, run)| ForwardStat {
                id: id.clone(),
                bytes_in: run.counters.bytes_in.load(Ordering::Relaxed),
                bytes_out: run.counters.bytes_out.load(Ordering::Relaxed),
            })
            .collect()
    }
}

#[derive(sqlx::FromRow)]
struct ForwardRuleRow {
    id: String,
    host_id: String,
    #[sqlx(rename = "type")]
    kind: String,
    bind_host: String,
    bind_port: i64,
    target_host: Option<String>,
    target_port: Option<i64>,
    auto_start: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardRuleView {
    pub id: String,
    pub host_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: Option<String>,
    pub target_port: Option<u16>,
    pub auto_start: bool,
    pub status: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardStat {
    pub id: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

async fn load_rule(db: &sqlx::SqlitePool, id: &str) -> Result<ForwardRuleRow, String> {
    sqlx::query_as::<_, ForwardRuleRow>(
        "SELECT id, host_id, type, bind_host, bind_port, target_host, target_port, auto_start
         FROM forward_rule WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "转发规则不存在，可能已被删除".to_string())
}

/// 规则列表：返回库中规则并附上运行状态与实时流量
#[tauri::command]
pub async fn forward_list(
    state: State<'_, AppState>,
    host_id: Option<String>,
) -> Result<Vec<ForwardRuleView>, String> {
    let rows = sqlx::query_as::<_, ForwardRuleRow>(
        "SELECT id, host_id, type, bind_host, bind_port, target_host, target_port, auto_start
         FROM forward_rule ORDER BY created_at ASC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let filter = host_id.filter(|h| !h.is_empty());
    let mut views = Vec::new();
    for row in rows {
        if let Some(host) = &filter {
            if &row.host_id != host {
                continue;
            }
        }
        let info = state.forwards.info(&row.id).await;
        let (bytes_in, bytes_out, status, error) = match info {
            Some((i, o, s, e)) => (i, o, s, e),
            None => (0, 0, "stopped".to_string(), None),
        };
        views.push(ForwardRuleView {
            id: row.id,
            host_id: row.host_id,
            kind: row.kind,
            bind_host: row.bind_host,
            bind_port: row.bind_port as u16,
            target_host: row.target_host,
            target_port: row.target_port.map(|p| p as u16),
            auto_start: row.auto_start != 0,
            status,
            bytes_in,
            bytes_out,
            error,
        });
    }
    Ok(views)
}

/// 保存规则：id 为空新增，否则更新；运行中禁止改监听地址/端口/类型
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn forward_save(
    state: State<'_, AppState>,
    id: Option<String>,
    host_id: String,
    forward_type: String,
    bind_host: String,
    bind_port: u16,
    target_host: Option<String>,
    target_port: Option<u16>,
    auto_start: bool,
) -> Result<String, String> {
    let kind = forward_type.trim().to_string();
    if !matches!(kind.as_str(), "local" | "dynamic" | "remote") {
        return Err("不支持的转发类型".to_string());
    }
    let bind_host = bind_host.trim();
    if bind_host.is_empty() {
        return Err("请填写监听地址".to_string());
    }
    if bind_port == 0 {
        return Err("监听端口需在 1–65535 之间".to_string());
    }

    let target_host = target_host.map(|h| h.trim().to_string()).filter(|h| !h.is_empty());
    let target_port = target_port.filter(|p| *p > 0);
    // 动态转发没有固定目标
    let (target_host, target_port) = if kind == "dynamic" {
        (None, None)
    } else {
        if target_host.is_none() {
            return Err("请填写目标地址".to_string());
        }
        let port = target_port.ok_or_else(|| "请填写目标端口".to_string())?;
        (target_host, Some(port as i64))
    };

    let now = now_secs();
    let existing_id = id.filter(|v| !v.is_empty());

    if let Some(existing) = existing_id.as_deref() {
        let old = load_rule(&state.db, existing).await?;
        if state.forwards.info(existing).await.is_some() {
            let changed = old.kind != kind
                || old.bind_host != bind_host
                || old.bind_port != bind_port as i64;
            if changed {
                return Err("规则正在运行，请先停止再修改类型或监听地址/端口".to_string());
            }
        }
        sqlx::query(
            "UPDATE forward_rule
             SET host_id = ?1, type = ?2, bind_host = ?3, bind_port = ?4,
                 target_host = ?5, target_port = ?6, auto_start = ?7, updated_at = ?8
             WHERE id = ?9",
        )
        .bind(&host_id)
        .bind(&kind)
        .bind(bind_host)
        .bind(bind_port as i64)
        .bind(&target_host)
        .bind(target_port)
        .bind(auto_start as i64)
        .bind(now)
        .bind(existing)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        Ok(existing.to_string())
    } else {
        let new_id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO forward_rule
               (id, host_id, type, bind_host, bind_port, target_host, target_port,
                auto_start, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        )
        .bind(&new_id)
        .bind(&host_id)
        .bind(&kind)
        .bind(bind_host)
        .bind(bind_port as i64)
        .bind(&target_host)
        .bind(target_port)
        .bind(auto_start as i64)
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        Ok(new_id)
    }
}

/// 删除规则：先停止再删库
#[tauri::command]
pub async fn forward_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if let Some(run) = state.forwards.take(&id).await {
        shutdown(state.forwards.registry.clone(), run).await;
    }
    sqlx::query("DELETE FROM forward_rule WHERE id = ?1")
        .bind(&id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 启动规则；密码可复用终端已建立的连接
#[tauri::command]
pub async fn forward_start(
    state: State<'_, AppState>,
    id: String,
    password: Option<String>,
) -> Result<(), String> {
    // 已有运行实例：连接还活着就拒绝重复启动，连接已断则先清理再重启
    if let Some(existing) = state.forwards.take(&id).await {
        if !existing.conn.handle.is_closed() {
            state.forwards.put(id.clone(), existing).await;
            return Err("规则已在运行".to_string());
        }
        shutdown(state.forwards.registry.clone(), existing).await;
    }

    let rule = load_rule(&state.db, &id).await?;
    let conn = state
        .sessions
        .acquire(&rule.host_id, password.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let counters = Arc::new(Counters::default());
    let error = Arc::new(StdMutex::new(None));
    let bind_port = rule.bind_port as u16;

    let running = match rule.kind.as_str() {
        "local" | "dynamic" => {
            let listener = TcpListener::bind((rule.bind_host.as_str(), bind_port))
                .await
                .map_err(|e| {
                    format!(
                        "监听 {}:{} 失败（端口可能已被占用）: {e}",
                        rule.bind_host, bind_port
                    )
                })?;
            let local_addr = listener
                .local_addr()
                .map(|a| a.port())
                .unwrap_or(bind_port);

            let target = if rule.kind == "local" {
                Some((
                    rule.target_host.clone().unwrap_or_default(),
                    rule.target_port.unwrap_or(0) as u16,
                ))
            } else {
                None
            };

            let task = if rule.kind == "local" {
                tokio::spawn(local_accept_loop(
                    listener,
                    conn.clone(),
                    target.clone().unwrap(),
                    counters.clone(),
                    error.clone(),
                ))
            } else {
                tokio::spawn(socks_accept_loop(
                    listener,
                    conn.clone(),
                    counters.clone(),
                    error.clone(),
                ))
            };
            log::info!("端口转发已启动: {} {}:{}", rule.kind, rule.bind_host, local_addr);
            RunningForward {
                kind: rule.kind.clone(),
                bind_host: rule.bind_host.clone(),
                bound_port: local_addr,
                conn,
                counters,
                tasks: vec![task],
                error,
            }
        }
        "remote" => {
            let target_host = rule
                .target_host
                .clone()
                .ok_or_else(|| "远程转发缺少目标地址".to_string())?;
            let target_port = rule.target_port.unwrap_or(0) as u16;
            let bound = conn
                .handle
                .tcpip_forward(rule.bind_host.clone(), bind_port as u32)
                .await
                .map_err(|e| match e {
                    russh::Error::RequestDenied => format!(
                        "服务器拒绝远程转发 {}:{}（sshd 可能禁用了 tcpip-forward 或端口已被占用）",
                        rule.bind_host, bind_port
                    ),
                    other => format!("请求远程转发 {}:{} 失败: {other}", rule.bind_host, bind_port),
                })?;
            let bound_port = if bound == 0 { bind_port } else { bound as u16 };
            state
                .forwards
                .registry
                .insert(
                    bound_port,
                    Arc::new(ForwardTarget {
                        host: target_host.clone(),
                        port: target_port,
                        counters: counters.clone(),
                    }),
                )
                .await;
            log::info!(
                "远程转发已启动: {}:{} → {}:{}",
                rule.bind_host,
                bound_port,
                target_host,
                target_port
            );
            RunningForward {
                kind: rule.kind.clone(),
                bind_host: rule.bind_host.clone(),
                bound_port,
                conn,
                counters,
                tasks: Vec::new(),
                error,
            }
        }
        other => return Err(format!("不支持的转发类型: {other}")),
    };

    state.forwards.put(id, running).await;
    Ok(())
}

/// 停止规则：释放监听端口/取消远程转发，并关闭所有活跃通道
#[tauri::command]
pub async fn forward_stop(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let Some(run) = state.forwards.take(&id).await else {
        return Ok(());
    };
    shutdown(state.forwards.registry.clone(), run).await;
    Ok(())
}

async fn shutdown(registry: Arc<ForwardRegistry>, run: RunningForward) {
    for task in run.tasks {
        task.abort();
        // 等任务真正退出，确保监听 socket 已释放
        let _ = task.await;
    }
    if run.kind == "remote" {
        if let Err(e) = run
            .conn
            .handle
            .cancel_tcpip_forward(run.bind_host.clone(), run.bound_port as u32)
            .await
        {
            log::warn!(
                "取消远程转发 {}:{} 失败: {e}",
                run.bind_host,
                run.bound_port
            );
        }
        registry.remove(run.bound_port).await;
    }
    log::info!("端口转发已停止: {}:{}", run.bind_host, run.bound_port);
}

/// 每 1 s 推送一次运行中规则的流量；前端丢弃 Channel 后循环自动退出
#[tauri::command]
pub async fn forward_stats(
    state: State<'_, AppState>,
    on_stats: Channel<Vec<ForwardStat>>,
) -> Result<(), String> {
    loop {
        if on_stats.send(state.forwards.snapshot().await).is_err() {
            break;
        }
        tokio::time::sleep(STATS_INTERVAL).await;
    }
    Ok(())
}

/// 本地转发：监听循环
async fn local_accept_loop(
    listener: TcpListener,
    conn: Arc<HostConnection>,
    target: (String, u16),
    counters: Arc<Counters>,
    error: Arc<StdMutex<Option<String>>>,
) {
    let mut tasks = JoinSet::new();
    let desc = format!("{}", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let conn = conn.clone();
                let counters = counters.clone();
                let (host, port) = target.clone();
                tasks.spawn(async move {
                    match conn
                        .handle
                        .channel_open_direct_tcpip(host.clone(), port as u32, "127.0.0.1", 0)
                        .await
                    {
                        Ok(channel) => {
                            log::info!("转发连接建立: {peer} → {host}:{port}");
                            bridge(stream, channel.into_stream(), counters).await;
                            log::info!("转发连接关闭: {peer}");
                        }
                        Err(e) => log::warn!("打开转发通道失败 {peer} → {host}:{port}: {e}"),
                    }
                });
            }
            Err(e) => {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(format!("监听 {desc} 出错: {e}"));
                }
                log::warn!("转发监听循环退出 ({desc}): {e}");
                break;
            }
        }
        while tasks.try_join_next().is_some() {}
    }
}

/// 动态转发：先完成 SOCKS5 握手，再按目标开 direct-tcpip
async fn socks_accept_loop(
    listener: TcpListener,
    conn: Arc<HostConnection>,
    counters: Arc<Counters>,
    error: Arc<StdMutex<Option<String>>>,
) {
    let mut tasks = JoinSet::new();
    let desc = format!("{}", listener.local_addr().map(|a| a.to_string()).unwrap_or_default());
    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                let conn = conn.clone();
                let counters = counters.clone();
                tasks.spawn(async move {
                    handle_socks_conn(stream, peer, conn, counters).await;
                });
            }
            Err(e) => {
                if let Ok(mut slot) = error.lock() {
                    *slot = Some(format!("监听 {desc} 出错: {e}"));
                }
                log::warn!("SOCKS5 监听循环退出 ({desc}): {e}");
                break;
            }
        }
        while tasks.try_join_next().is_some() {}
    }
}

async fn handle_socks_conn(
    mut stream: TcpStream,
    peer: std::net::SocketAddr,
    conn: Arc<HostConnection>,
    counters: Arc<Counters>,
) {
    let (host, port) = match socks5_handshake(&mut stream).await {
        Ok(target) => target,
        Err(reply) => {
            if let Some(code) = reply {
                let _ = socks5_reply(&mut stream, code).await;
            }
            return;
        }
    };
    match conn
        .handle
        .channel_open_direct_tcpip(host.clone(), port as u32, "127.0.0.1", 0)
        .await
    {
        Ok(channel) => {
            if stream.write_all(&SOCKS5_OK).await.is_err() {
                return;
            }
            log::info!("SOCKS5 连接建立: {peer} → {host}:{port}");
            bridge(stream, channel.into_stream(), counters).await;
            log::info!("SOCKS5 连接关闭: {peer}");
        }
        Err(e) => {
            log::warn!("SOCKS5 打开通道失败 {peer} → {host}:{port}: {e}");
            let _ = socks5_reply(&mut stream, 0x05).await;
        }
    }
}

/// 完成 SOCKS5 方法协商与 CONNECT 请求；错误时返回应回给客户端的 REP 码
async fn socks5_handshake(stream: &mut TcpStream) -> Result<(String, u16), Option<u8>> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await.map_err(|_| None)?;
    if head[0] != 0x05 {
        return Err(None);
    }
    let mut methods = vec![0u8; head[1] as usize];
    stream.read_exact(&mut methods).await.map_err(|_| None)?;
    if !methods.contains(&0x00) {
        return Err(Some(0xFF));
    }
    stream
        .write_all(&[0x05, 0x00])
        .await
        .map_err(|_| None)?;

    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await.map_err(|_| None)?;
    if req[0] != 0x05 {
        return Err(None);
    }
    if req[1] != 0x01 {
        return Err(Some(0x07));
    }

    let host = match req[3] {
        0x01 => {
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).await.map_err(|_| None)?;
            Ipv4Addr::from(buf).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(|_| None)?;
            let mut buf = vec![0u8; len[0] as usize];
            stream.read_exact(&mut buf).await.map_err(|_| None)?;
            String::from_utf8(buf).map_err(|_| Some(0x04))?
        }
        0x04 => {
            let mut buf = [0u8; 16];
            stream.read_exact(&mut buf).await.map_err(|_| None)?;
            Ipv6Addr::from(buf).to_string()
        }
        _ => return Err(Some(0x08)),
    };

    let mut port_buf = [0u8; 2];
    stream.read_exact(&mut port_buf).await.map_err(|_| None)?;
    Ok((host, u16::from_be_bytes(port_buf)))
}

async fn socks5_reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    let reply = [0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    stream.write_all(&reply).await
}

/// 双向桥接并分别累计入/出字节
pub(crate) async fn bridge<A, B>(a: A, b: B, counters: Arc<Counters>)
where
    A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    B: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut a_rx, mut a_tx) = tokio::io::split(a);
    let (mut b_rx, mut b_tx) = tokio::io::split(b);
    let out_counters = counters.clone();

    let up = async move {
        let _ = copy_count(&mut a_rx, &mut b_tx, &out_counters.bytes_out).await;
        let _ = b_tx.shutdown().await;
    };
    let down = async move {
        let _ = copy_count(&mut b_rx, &mut a_tx, &counters.bytes_in).await;
        let _ = a_tx.shutdown().await;
    };
    tokio::join!(up, down);
}

async fn copy_count<R, W>(reader: &mut R, writer: &mut W, counter: &AtomicU64) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; COPY_BUFFER];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await?;
        total += n as u64;
        counter.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(total)
}
