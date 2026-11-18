use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::RwLock;

use crate::commands::AppState;

/// 攒批周期与阈值：8 ms 或 64 KB 触发一次 IPC
const BATCH_INTERVAL: Duration = Duration::from_millis(8);
const BATCH_BYTES: usize = 64 * 1024;

/// 单个终端标签的写半部，与读循环解耦
pub struct TermEntry {
    write: Arc<ChannelWriteHalf<Msg>>,
}

/// tab_id → 终端通道
#[derive(Default)]
pub struct TerminalManager {
    map: RwLock<HashMap<String, Arc<TermEntry>>>,
}

impl TerminalManager {
    async fn insert(&self, tab_id: String, entry: Arc<TermEntry>) {
        self.map.write().await.insert(tab_id, entry);
    }

    async fn get(&self, tab_id: &str) -> Option<Arc<TermEntry>> {
        self.map.read().await.get(tab_id).cloned()
    }

    async fn remove(&self, tab_id: &str) {
        self.map.write().await.remove(tab_id);
    }
}

#[tauri::command]
pub async fn term_open(
    state: State<'_, AppState>,
    tab_id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    password: Option<String>,
    on_data: Channel<Vec<u8>>,
) -> Result<(), String> {
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
        .request_pty(
            true,
            "xterm-256color",
            cols as u32,
            rows as u32,
            0,
            0,
            &[],
        )
        .await
        .map_err(|e| format!("申请 PTY 失败: {e}"))?;

    channel
        .request_shell(true)
        .await
        .map_err(|e| format!("启动远端 shell 失败: {e}"))?;

    let (read_half, write_half) = channel.split();
    let entry = Arc::new(TermEntry {
        write: Arc::new(write_half),
    });
    state.terminals.insert(tab_id.clone(), entry).await;

    let result = pump(read_half, &on_data).await;
    state.terminals.remove(&tab_id).await;
    result.map_err(|e| e.to_string())
}

/// 读循环：攒批后推送给前端，直到远端关闭通道
async fn pump(
    mut read: russh::ChannelReadHalf,
    on_data: &Channel<Vec<u8>>,
) -> anyhow::Result<()> {
    let mut batch: Vec<u8> = Vec::with_capacity(BATCH_BYTES);
    let mut ticker = tokio::time::interval(BATCH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            msg = read.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    batch.extend_from_slice(&data);
                    if batch.len() >= BATCH_BYTES {
                        flush(on_data, &mut batch)?;
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    batch.extend_from_slice(&data);
                    if batch.len() >= BATCH_BYTES {
                        flush(on_data, &mut batch)?;
                    }
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                _ => {}
            },
            _ = ticker.tick() => {
                if !batch.is_empty() {
                    flush(on_data, &mut batch)?;
                }
            }
        }
    }

    flush(on_data, &mut batch)?;
    Ok(())
}

fn flush(on_data: &Channel<Vec<u8>>, batch: &mut Vec<u8>) -> anyhow::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let data = std::mem::replace(batch, Vec::with_capacity(BATCH_BYTES));
    on_data
        .send(data)
        .map_err(|e| anyhow!("终端数据推送失败: {e}"))
}

#[tauri::command]
pub async fn term_write(
    state: State<'_, AppState>,
    tab_id: String,
    data: String,
) -> Result<(), String> {
    let entry = state
        .terminals
        .get(&tab_id)
        .await
        .ok_or_else(|| "终端会话不存在".to_string())?;

    entry
        .write
        .data_bytes(data.into_bytes())
        .await
        .context("写入终端失败")
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn term_resize(
    state: State<'_, AppState>,
    tab_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let entry = state
        .terminals
        .get(&tab_id)
        .await
        .ok_or_else(|| "终端会话不存在".to_string())?;

    entry
        .write
        .window_change(cols as u32, rows as u32, 0, 0)
        .await
        .context("调整终端尺寸失败")
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn term_close(state: State<'_, AppState>, tab_id: String) -> Result<(), String> {
    let Some(entry) = state.terminals.get(&tab_id).await else {
        return Ok(());
    };

    if let Err(e) = entry.write.close().await {
        log::warn!("关闭终端通道失败: {e}");
    }
    state.terminals.remove(&tab_id).await;
    drop(entry);
    Ok(())
}
