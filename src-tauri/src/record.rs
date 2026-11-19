//! 终端日志录制：把远端输出写成本地文本文件。
//!
//! 落盘前会剥离 ANSI 控制序列，避免日志里出现颜色码与光标控制字符，
//! 同时保留换行、回车和制表符，便于直接阅读。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, RwLock};
use tauri::State;

use crate::commands::AppState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StripState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

/// 跨数据块的 ANSI 序列剥离器：转义序列可能被拆到两次输出里
#[derive(Default)]
struct AnsiStripper {
    state: StripState,
}

impl AnsiStripper {
    fn strip(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &byte in input {
            match self.state {
                StripState::Ground => match byte {
                    0x1b => self.state = StripState::Escape,
                    b'\n' | b'\r' | b'\t' => out.push(byte),
                    // 其余 C0 控制字符（响铃、退格等）不写入日志
                    0x00..=0x1f => {}
                    _ => out.push(byte),
                },
                StripState::Escape => {
                    self.state = match byte {
                        b'[' => StripState::Csi,
                        b']' => StripState::Osc,
                        _ => StripState::Ground,
                    };
                }
                StripState::Csi => {
                    // CSI 以 @ ~ 之间的最终字节结束
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = StripState::Ground;
                    }
                }
                StripState::Osc => match byte {
                    0x07 => self.state = StripState::Ground,
                    0x1b => self.state = StripState::OscEscape,
                    _ => {}
                },
                StripState::OscEscape => {
                    self.state = match byte {
                        b'\\' => StripState::Ground,
                        // 不是 ST，回到 OSC 继续吞
                        _ => StripState::Osc,
                    };
                }
            }
        }
    }
}

/// 单个标签的录制器：持有日志文件与跨块剥离状态
pub struct Recorder {
    file: File,
    path: PathBuf,
    stripper: AnsiStripper,
    closed: bool,
}

impl Recorder {
    /// 写入一段远端输出；自动剥离 ANSI 控制序列
    pub async fn record(&mut self, bytes: &[u8]) {
        if self.closed || bytes.is_empty() {
            return;
        }
        let mut text = Vec::with_capacity(bytes.len());
        self.stripper.strip(bytes, &mut text);
        if text.is_empty() {
            return;
        }
        if let Err(e) = self.file.write_all(&text).await {
            log::warn!("写入终端日志失败 {}: {e}", self.path.display());
        }
    }

    async fn close(&mut self) {
        self.closed = true;
        if let Err(e) = self.file.flush().await {
            log::warn!("刷新终端日志失败 {}: {e}", self.path.display());
        }
    }
}

/// 一个标签的录制状态：所属窗口 + 录制器
struct ActiveRecording {
    window: String,
    recorder: Arc<Mutex<Recorder>>,
}

/// tab_id → 录制；跨重连保持，直到用户停止或关闭标签/窗口
#[derive(Default)]
pub struct RecordingManager {
    map: RwLock<HashMap<String, ActiveRecording>>,
}

impl RecordingManager {
    /// 开始录制；同一标签已有录制时先关闭旧文件
    pub async fn start(&self, tab_id: &str, path: &Path, window: &str) -> Result<PathBuf, String> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(|e| format!("创建日志文件失败: {e}"))?;

        let recorder = ActiveRecording {
            window: window.to_string(),
            recorder: Arc::new(Mutex::new(Recorder {
                file,
                path: path.to_path_buf(),
                stripper: AnsiStripper::default(),
                closed: false,
            })),
        };

        let mut map = self.map.write().await;
        if let Some(old) = map.insert(tab_id.to_string(), recorder) {
            old.recorder.lock().await.close().await;
        }
        Ok(path.to_path_buf())
    }

    pub async fn get(&self, tab_id: &str) -> Option<Arc<Mutex<Recorder>>> {
        self.map
            .read()
            .await
            .get(tab_id)
            .map(|active| active.recorder.clone())
    }

    /// 停止录制并返回日志路径；未在录制时返回 None
    pub async fn stop(&self, tab_id: &str) -> Option<PathBuf> {
        let active = self.map.write().await.remove(tab_id)?;
        let recorder = active.recorder;
        let mut guard = recorder.lock().await;
        guard.close().await;
        Some(guard.path.clone())
    }

    /// 关闭某个窗口下的全部录制，退出前刷新文件
    pub async fn close_window(&self, window: &str) {
        let targets: Vec<ActiveRecording> = {
            let mut map = self.map.write().await;
            let tab_ids: Vec<String> = map
                .iter()
                .filter(|(_, active)| active.window == window)
                .map(|(tab_id, _)| tab_id.clone())
                .collect();
            tab_ids
                .into_iter()
                .filter_map(|tab_id| map.remove(&tab_id))
                .collect()
        };

        for active in targets {
            active.recorder.lock().await.close().await;
        }
    }
}

/// 开始录制指定终端标签；path 为用户选择的本地文件
#[tauri::command]
pub async fn term_log_start(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    tab_id: String,
    path: String,
) -> Result<String, String> {
    state
        .recordings
        .start(&tab_id, Path::new(&path), window.label())
        .await
        .map(|path| path.display().to_string())
}

/// 停止录制并返回日志路径；未在录制时返回 null
#[tauri::command]
pub async fn term_log_stop(
    state: State<'_, AppState>,
    tab_id: String,
) -> Result<Option<String>, String> {
    Ok(state
        .recordings
        .stop(&tab_id)
        .await
        .map(|path| path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::AnsiStripper;

    fn strip(input: &[u8]) -> String {
        let mut out = Vec::new();
        AnsiStripper::default().strip(input, &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn removes_color_and_cursor_sequences() {
        let input = b"\x1b[31mred\x1b[0m\tok\r\n\x1b[2Kdone";
        assert_eq!(strip(input), "red\tok\r\ndone");
    }

    #[test]
    fn removes_osc_title_and_bell() {
        let input = b"\x1b]0;title\x07hello\x1b]2;x\x1b\\world";
        assert_eq!(strip(input), "helloworld");
    }

    #[test]
    fn keeps_state_across_chunks() {
        let mut stripper = AnsiStripper::default();
        let mut out = Vec::new();
        stripper.strip(b"a\x1b[3", &mut out);
        stripper.strip(b"1mb", &mut out);
        stripper.strip(b"\x1b]0;ti", &mut out);
        stripper.strip(b"tle\x07c", &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "abc");
    }
}
