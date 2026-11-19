use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::{Mutex, Notify, RwLock};

use crate::commands::AppState;
use crate::record::Recorder;

/// 攒批周期与阈值：8 ms 或 64 KB 触发一次 IPC
const BATCH_INTERVAL: Duration = Duration::from_millis(8);
const BATCH_BYTES: usize = 64 * 1024;

/// 单个终端标签：连接建立前即可取消，写半部在 PTY 就绪后填入
pub struct TermEntry {
    write: RwLock<Option<Arc<ChannelWriteHalf<Msg>>>>,
    cancel: Notify,
    /// 所属窗口标签，用于窗口关闭时清理
    window: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TermExit {
    ShellExit,
    Disconnected,
    Closed,
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

    /// 关闭某个窗口下的全部终端，避免多窗口关闭后残留远端 shell
    pub async fn close_window(&self, window: &str) {
        let targets: Vec<(String, Arc<TermEntry>)> = {
            let map = self.map.read().await;
            map.iter()
                .filter(|(_, entry)| entry.window == window)
                .map(|(tab_id, entry)| (tab_id.clone(), entry.clone()))
                .collect()
        };

        for (tab_id, entry) in targets {
            entry.cancel.notify_one();
            if let Some(write) = entry.write.write().await.take() {
                if let Err(e) = write.close().await {
                    log::warn!("关闭窗口 {window} 的终端通道失败: {e}");
                }
            }
            self.remove(&tab_id).await;
        }
    }
}

#[tauri::command]
pub async fn term_open(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    tab_id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    password: Option<String>,
    on_data: Channel<Vec<u8>>,
) -> Result<TermExit, String> {
    // 在 acquire 前注册，连接在途时也能通过 term_close 取消
    let entry = Arc::new(TermEntry {
        write: RwLock::new(None),
        cancel: Notify::new(),
        window: window.label().to_string(),
    });
    state.terminals.insert(tab_id.clone(), entry.clone()).await;
    // 录制跨重连保持：每次 attempt 重新取一次当前录制器
    let recorder = state.recordings.get(&tab_id).await;

    let result = tokio::select! {
        _ = entry.cancel.notified() => Ok(TermExit::Closed),
        result = async {
            let conn = state.sessions.acquire(&host_id, password.as_deref()).await?;
            let channel = conn
                .handle
                .channel_open_session()
                .await
                .map_err(|e| anyhow!("打开会话通道失败: {e}"))?;

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
                .map_err(|e| anyhow!("申请 PTY 失败: {e}"))?;
            channel
                .request_shell(true)
                .await
                .map_err(|e| anyhow!("启动远端 shell 失败: {e}"))?;

            let (read_half, write_half) = channel.split();
            *entry.write.write().await = Some(Arc::new(write_half));
            pump(read_half, &on_data, recorder).await
        } => result,
    };

    state.terminals.remove(&tab_id).await;
    result.map_err(|e| e.to_string())
}

/// 读循环：攒批后推送给前端，直到远端关闭通道
async fn pump(
    mut read: russh::ChannelReadHalf,
    on_data: &Channel<Vec<u8>>,
    recorder: Option<Arc<Mutex<Recorder>>>,
) -> anyhow::Result<TermExit> {
    let mut batch: Vec<u8> = Vec::with_capacity(BATCH_BYTES);
    let mut ticker = tokio::time::interval(BATCH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut exit_status = None;
    let mut exit_signal = false;

    loop {
        tokio::select! {
            msg = read.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    batch.extend_from_slice(&data);
                    if batch.len() >= BATCH_BYTES {
                        flush(on_data, &mut batch, recorder.as_ref()).await?;
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    batch.extend_from_slice(&data);
                    if batch.len() >= BATCH_BYTES {
                        flush(on_data, &mut batch, recorder.as_ref()).await?;
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status: status }) => {
                    exit_status = Some(status);
                }
                Some(ChannelMsg::ExitSignal { .. }) => exit_signal = true,
                // Eof 后仍等待 Close，避免漏掉稍晚到达的退出状态
                Some(ChannelMsg::Eof) => {}
                Some(ChannelMsg::Close) | None => break,
                _ => {}
            },
            _ = ticker.tick() => {
                if !batch.is_empty() {
                    flush(on_data, &mut batch, recorder.as_ref()).await?;
                }
            }
        }
    }

    flush(on_data, &mut batch, recorder.as_ref()).await?;
    Ok(classify_exit(exit_status, exit_signal))
}

fn classify_exit(exit_status: Option<u32>, exit_signal: bool) -> TermExit {
    if exit_status.is_some() && !exit_signal {
        TermExit::ShellExit
    } else {
        TermExit::Disconnected
    }
}

async fn flush(
    on_data: &Channel<Vec<u8>>,
    batch: &mut Vec<u8>,
    recorder: Option<&Arc<Mutex<Recorder>>>,
) -> anyhow::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let data = std::mem::replace(batch, Vec::with_capacity(BATCH_BYTES));
    if let Some(recorder) = recorder {
        recorder.lock().await.record(&data).await;
    }
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

    let write = entry
        .write
        .read()
        .await
        .clone()
        .ok_or_else(|| "终端尚未就绪".to_string())?;
    write
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

    let write = entry
        .write
        .read()
        .await
        .clone()
        .ok_or_else(|| "终端尚未就绪".to_string())?;
    write
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

    entry.cancel.notify_one();
    let write = entry.write.write().await.take();
    if let Some(write) = write {
        if let Err(e) = write.close().await {
            log::warn!("关闭终端通道失败: {e}");
        }
    }
    state.terminals.remove(&tab_id).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{classify_exit, TermExit};

    #[test]
    fn classifies_shell_and_abnormal_exit() {
        assert_eq!(classify_exit(Some(0), false), TermExit::ShellExit);
        assert_eq!(classify_exit(Some(1), false), TermExit::ShellExit);
        assert_eq!(classify_exit(Some(0), true), TermExit::Disconnected);
        assert_eq!(classify_exit(None, false), TermExit::Disconnected);
    }
}
