use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{Mutex, RwLock};

use crate::commands::AppState;

/// 传输分块大小与进度推送节流间隔
const CHUNK_SIZE: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

/// SFTP 会话与传输取消标志
#[derive(Default)]
pub struct SftpManager {
    /// tab_id → SFTP 会话（每个标签独立通道）
    sessions: RwLock<HashMap<String, Arc<SftpSession>>>,
    /// transfer_id → 取消标志
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl SftpManager {
    async fn insert(&self, tab_id: String, session: Arc<SftpSession>) {
        self.sessions.write().await.insert(tab_id, session);
    }

    async fn take(&self, tab_id: &str) -> Option<Arc<SftpSession>> {
        self.sessions.write().await.remove(tab_id)
    }

    async fn session(&self, tab_id: &str) -> Result<Arc<SftpSession>, String> {
        self.sessions
            .read()
            .await
            .get(tab_id)
            .cloned()
            .ok_or_else(|| "SFTP 会话不存在，请重新打开".to_string())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpOpenResult {
    pub home: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mode: u32,
    pub mtime: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transferred: u64,
    pub total: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDir {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<LocalEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: i64,
}

/// 打开 SFTP 会话：复用已建立的 SSH 连接，再申请 sftp 子系统
#[tauri::command]
pub async fn sftp_open(
    state: State<'_, AppState>,
    tab_id: String,
    host_id: String,
    password: Option<String>,
) -> Result<SftpOpenResult, String> {
    // 同一标签重复打开时先释放旧会话，避免通道泄漏耗尽 MaxSessions
    if let Some(old) = state.sftp.take(&tab_id).await {
        let _ = tokio::time::timeout(Duration::from_secs(2), old.close()).await;
    }

    let conn = state
        .sessions
        .acquire(&host_id, password.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let channel = conn
        .handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开会话通道失败: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("启动 SFTP 子系统失败: {e}"))?;

    let session = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP 协议协商失败: {e}"))?;
    let home = session
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| "/".to_string());

    state.sftp.insert(tab_id, Arc::new(session)).await;
    log::info!("SFTP 会话已建立，home={home}");
    Ok(SftpOpenResult { home })
}

/// 关闭 SFTP 会话：先移除引用再关闭通道，2 s 超时兜底
#[tauri::command]
pub async fn sftp_close(state: State<'_, AppState>, tab_id: String) -> Result<(), String> {
    let Some(session) = state.sftp.take(&tab_id).await else {
        return Ok(());
    };

    match tokio::time::timeout(Duration::from_secs(2), session.close()).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => log::warn!("关闭 SFTP 会话失败: {e}"),
        Err(_) => log::warn!("关闭 SFTP 会话超时: {tab_id}"),
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    tab_id: String,
    path: String,
) -> Result<Vec<SftpEntry>, String> {
    let session = state.sftp.session(&tab_id).await?;
    let read_dir = session
        .read_dir(&path)
        .await
        .map_err(|e| format!("读取目录失败: {e}"))?;

    let mut entries = Vec::new();
    for entry in read_dir {
        let name = entry.file_name();
        let meta = entry.metadata();
        let link = entry.path();
        let is_symlink = meta.is_symlink();
        let mut is_dir = meta.is_dir();
        let mut size = meta.size.unwrap_or(0);

        // 符号链接尽量解析目标类型，便于双击进入目录
        if is_symlink {
            if let Ok(target) = session.metadata(&link).await {
                is_dir = target.is_dir();
                if !is_dir {
                    size = target.size.unwrap_or(size);
                }
            }
        }

        entries.push(SftpEntry {
            name,
            path: link,
            is_dir,
            is_symlink,
            size,
            mode: meta.permissions.unwrap_or(0) & 0o7777,
            mtime: meta.mtime.unwrap_or(0) as i64,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    tab_id: String,
    path: String,
) -> Result<(), String> {
    let session = state.sftp.session(&tab_id).await?;
    session
        .create_dir(&path)
        .await
        .map_err(|e| format!("新建目录失败: {e}"))
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    tab_id: String,
    from: String,
    to: String,
) -> Result<(), String> {
    let session = state.sftp.session(&tab_id).await?;
    session
        .rename(&from, &to)
        .await
        .map_err(|e| format!("重命名失败: {e}"))
}

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    tab_id: String,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let session = state.sftp.session(&tab_id).await?;
    if is_dir {
        session
            .remove_dir(&path)
            .await
            .map_err(|e| format!("删除目录失败（非空目录请先清空）: {e}"))
    } else {
        session
            .remove_file(&path)
            .await
            .map_err(|e| format!("删除文件失败: {e}"))
    }
}

#[tauri::command]
pub async fn sftp_chmod(
    state: State<'_, AppState>,
    tab_id: String,
    path: String,
    mode: u32,
) -> Result<(), String> {
    let session = state.sftp.session(&tab_id).await?;
    let mut attrs = FileAttributes::empty();
    attrs.permissions = Some(mode & 0o7777);
    session
        .set_metadata(&path, attrs)
        .await
        .map_err(|e| format!("修改权限失败: {e}"))
}

/// 下载：支持断点续传，取消后保留已下载部分
#[tauri::command]
pub async fn sftp_download(
    state: State<'_, AppState>,
    tab_id: String,
    transfer_id: String,
    remote: String,
    local: String,
    on_progress: Channel<TransferProgress>,
) -> Result<u64, String> {
    let session = state.sftp.session(&tab_id).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .sftp
        .cancels
        .lock()
        .await
        .insert(transfer_id.clone(), cancel.clone());

    let result = download_inner(&session, &remote, &local, &cancel, &on_progress).await;

    state.sftp.cancels.lock().await.remove(&transfer_id);
    result
}

/// 上传：支持断点续传，取消后保留已上传部分
#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, AppState>,
    tab_id: String,
    transfer_id: String,
    remote: String,
    local: String,
    on_progress: Channel<TransferProgress>,
) -> Result<u64, String> {
    let session = state.sftp.session(&tab_id).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    state
        .sftp
        .cancels
        .lock()
        .await
        .insert(transfer_id.clone(), cancel.clone());

    let result = upload_inner(&session, &remote, &local, &cancel, &on_progress).await;

    state.sftp.cancels.lock().await.remove(&transfer_id);
    result
}

#[tauri::command]
pub async fn sftp_cancel(state: State<'_, AppState>, transfer_id: String) -> Result<(), String> {
    if let Some(flag) = state.sftp.cancels.lock().await.get(&transfer_id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

async fn download_inner(
    session: &SftpSession,
    remote: &str,
    local: &str,
    cancel: &AtomicBool,
    on_progress: &Channel<TransferProgress>,
) -> Result<u64, String> {
    let meta = session
        .metadata(remote)
        .await
        .map_err(|e| format!("读取远端文件信息失败: {e}"))?;
    if meta.is_dir() {
        return Err("不能下载目录".to_string());
    }
    let total = meta.size.unwrap_or(0);
    let local_path = PathBuf::from(local);

    let local_len = tokio::fs::metadata(&local_path)
        .await
        .ok()
        .map(|m| m.len());
    let start = match local_len {
        Some(len) if len == total => {
            let _ = on_progress.send(TransferProgress {
                transferred: total,
                total,
            });
            return Ok(total);
        }
        // 长度一致才可续传；更长说明本地文件已损坏，从头覆盖
        Some(len) if len < total => len,
        _ => 0,
    };

    let mut remote_file = session
        .open(remote)
        .await
        .map_err(|e| format!("打开远端文件失败: {e}"))?;
    if start > 0 {
        remote_file
            .seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| format!("远端文件定位失败: {e}"))?;
    }

    let mut local_file = if start > 0 {
        OpenOptions::new()
            .append(true)
            .open(&local_path)
            .await
            .map_err(|e| format!("打开本地文件失败: {e}"))?
    } else {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&local_path)
            .await
            .map_err(|e| format!("创建本地文件失败: {e}"))?
    };

    let mut transferred = start;
    let _ = on_progress.send(TransferProgress {
        transferred,
        total,
    });

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut last = Instant::now();
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }

        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取远端数据失败: {e}"))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入本地文件失败: {e}"))?;
        transferred += n as u64;

        if last.elapsed() >= PROGRESS_INTERVAL {
            let _ = on_progress.send(TransferProgress {
                transferred,
                total,
            });
            last = Instant::now();
        }
    }

    local_file
        .flush()
        .await
        .map_err(|e| format!("写入本地文件失败: {e}"))?;
    drop(local_file);
    if let Err(e) = remote_file.close().await {
        log::warn!("关闭远端文件句柄失败: {e}");
    }

    if cancelled {
        let _ = on_progress.send(TransferProgress {
            transferred,
            total,
        });
        return Err("已取消".to_string());
    }

    let _ = on_progress.send(TransferProgress {
        transferred,
        total,
    });
    Ok(transferred)
}

async fn upload_inner(
    session: &SftpSession,
    remote: &str,
    local: &str,
    cancel: &AtomicBool,
    on_progress: &Channel<TransferProgress>,
) -> Result<u64, String> {
    let local_path = PathBuf::from(local);
    let local_meta = tokio::fs::metadata(&local_path)
        .await
        .map_err(|e| format!("读取本地文件失败: {e}"))?;
    if local_meta.is_dir() {
        return Err("不能上传目录".to_string());
    }
    let total = local_meta.len();

    let remote_len = if session
        .try_exists(remote)
        .await
        .map_err(|e| format!("读取远端文件信息失败: {e}"))?
    {
        Some(
            session
                .metadata(remote)
                .await
                .map_err(|e| format!("读取远端文件信息失败: {e}"))?
                .size
                .unwrap_or(0),
        )
    } else {
        None
    };

    let start = match remote_len {
        Some(len) if len == total => {
            let _ = on_progress.send(TransferProgress {
                transferred: total,
                total,
            });
            return Ok(total);
        }
        Some(len) if len < total => len,
        _ => 0,
    };

    let mut remote_file = if start > 0 {
        session
            .open_with_flags(remote, OpenFlags::WRITE | OpenFlags::CREATE)
            .await
    } else {
        session
            .open_with_flags(remote, OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE)
            .await
    }
    .map_err(|e| format!("打开远端文件失败: {e}"))?;
    if start > 0 {
        remote_file
            .seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| format!("远端文件定位失败: {e}"))?;
    }

    let mut local_file = tokio::fs::File::open(&local_path)
        .await
        .map_err(|e| format!("打开本地文件失败: {e}"))?;
    if start > 0 {
        local_file
            .seek(std::io::SeekFrom::Start(start))
            .await
            .map_err(|e| format!("本地文件定位失败: {e}"))?;
    }

    let mut transferred = start;
    let _ = on_progress.send(TransferProgress {
        transferred,
        total,
    });

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut last = Instant::now();
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }

        let n = local_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取本地文件失败: {e}"))?;
        if n == 0 {
            break;
        }
        remote_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入远端文件失败: {e}"))?;
        transferred += n as u64;

        if last.elapsed() >= PROGRESS_INTERVAL {
            let _ = on_progress.send(TransferProgress {
                transferred,
                total,
            });
            last = Instant::now();
        }
    }

    if let Err(e) = remote_file.flush().await {
        log::warn!("刷新远端文件失败: {e}");
    }
    if let Err(e) = remote_file.close().await {
        log::warn!("关闭远端文件句柄失败: {e}");
    }

    if cancelled {
        let _ = on_progress.send(TransferProgress {
            transferred,
            total,
        });
        return Err("已取消".to_string());
    }

    let _ = on_progress.send(TransferProgress {
        transferred,
        total,
    });
    Ok(transferred)
}

#[tauri::command]
pub async fn local_home() -> Result<String, String> {
    Ok(home_dir().display().to_string())
}

/// 本地目录浏览：path 为空时回到用户主目录
#[tauri::command]
pub async fn local_list(path: Option<String>) -> Result<LocalDir, String> {
    let dir = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home_dir(),
    };

    let mut reader = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("读取本地目录失败: {e}"))?;

    let mut entries = Vec::new();
    while let Some(entry) = reader
        .next_entry()
        .await
        .map_err(|e| format!("读取本地目录失败: {e}"))?
    {
        let entry_path = entry.path();
        // std 的 metadata 会跟随符号链接，目录判断更准确
        let meta = tokio::fs::metadata(&entry_path).await.ok();
        let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
        entries.push(LocalEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry_path.display().to_string(),
            is_dir,
            size: if is_dir {
                0
            } else {
                meta.as_ref().map(|m| m.len()).unwrap_or(0)
            },
            mtime: meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(unix_secs)
                .unwrap_or(0),
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(LocalDir {
        parent: dir.parent().map(|p| p.display().to_string()),
        path: dir.display().to_string(),
        entries,
    })
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn unix_secs(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}
