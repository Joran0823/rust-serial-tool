// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 串口会话：一个写线程负责所有写操作与长任务调度，
//! 每次打开串口额外启动一个读线程（通过 try_clone 共享句柄）。

use crate::config::{FileSendMode, PortSettings};
use crate::i18n::Language;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// UI → 会话命令
#[derive(Debug)]
pub enum Command {
    Open(PortSettings),
    SetLanguage(Language),
    Close,
    Write(Vec<u8>),
    StartPeriodic { bytes: Vec<u8>, interval_ms: u32 },
    StopPeriodic,
    StartQueue { name: String, items: Vec<QueueSendItem> },
    StopQueue,
    StartFile {
        path: PathBuf,
        mode: FileSendMode,
        line_ending: Vec<u8>,
        line_interval_ms: u32,
    },
    StopFile,
}

/// 队列发送条目（已预解析为字节 + 条目延迟）
#[derive(Debug, Clone)]
pub struct QueueSendItem {
    pub bytes: Vec<u8>,
    pub delay_ms: u32,
}

/// 会话 → UI 事件
#[derive(Debug)]
pub enum Event {
    Opened { port_name: String },
    Closed,
    Disconnected,
    Error(String),
    Data(Vec<u8>),
    Tx(Vec<u8>),
    Periodic(bool),
    QueueStarted { name: String, total: usize },
    QueueProgress { current: usize, total: usize },
    QueueDone,
    QueueStopped,
    FileStarted { name: String, total: u64 },
    FileProgress { sent: u64, total: u64 },
    FileDone,
    FileStopped,
}

pub struct SerialSession {
    cmd_tx: Sender<Command>,
    evt_rx: Receiver<Event>,
    _handle: JoinHandle<()>,
}

impl SerialSession {
    pub fn spawn(lang: Language) -> Self {
        let (cmd_tx, cmd_rx) = channel();
        let (evt_tx, evt_rx) = sync_channel(1024);
        let handle = std::thread::spawn(move || writer_loop(cmd_rx, evt_tx, lang));
        Self {
            cmd_tx,
            evt_rx,
            _handle: handle,
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn try_recv_event(&self) -> Option<Event> {
        self.evt_rx.try_recv().ok()
    }
}

struct PeriodicState {
    bytes: Vec<u8>,
    interval_ms: u32,
    next: Instant,
}

struct QueueState {
    items: Vec<QueueSendItem>,
    index: usize,
    wait_until: Option<Instant>,
}

struct FileState {
    name: String,
    data: Vec<u8>,
    mode: FileSendMode,
    line_ending: Vec<u8>,
    line_interval_ms: u32,
    lines: Vec<(usize, usize)>,
    line_idx: usize,
    pos: usize,
    total: usize,
    wait_until: Option<Instant>,
    last_report: Instant,
    last_report_pos: usize,
}

impl FileState {
    fn new(
        path: PathBuf,
        mode: FileSendMode,
        line_ending: Vec<u8>,
        line_interval_ms: u32,
        lang: Language,
    ) -> Result<Self, String> {
        const MAX_FILE: usize = 512 * 1024 * 1024;
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() > MAX_FILE {
            return Err(lang.strings().file_too_large.to_string());
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let mut lines = Vec::new();
        if mode == FileSendMode::LineByLine {
            let mut start = 0usize;
            for (i, &b) in data.iter().enumerate() {
                if b == b'\n' {
                    let mut end = i;
                    if end > start && data[end - 1] == b'\r' {
                        end -= 1;
                    }
                    lines.push((start, end));
                    start = i + 1;
                }
            }
            if start < data.len() {
                lines.push((start, data.len()));
            }
        }

        Ok(Self {
            name,
            total: data.len(),
            data,
            mode,
            line_ending,
            line_interval_ms,
            lines,
            line_idx: 0,
            pos: 0,
            wait_until: None,
            last_report: Instant::now() - Duration::from_secs(60),
            last_report_pos: 0,
        })
    }
}

fn writer_loop(cmd_rx: Receiver<Command>, evt_tx: SyncSender<Event>, mut lang: Language) {
    let mut port: Option<Box<dyn serialport::SerialPort>> = None;
    let mut reader_stop: Option<Arc<AtomicBool>> = None;
    let mut periodic: Option<PeriodicState> = None;
    let mut queue: Option<QueueState> = None;
    let mut file: Option<FileState> = None;

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Command::Open(settings) => {
                    if let Some(flag) = reader_stop.take() {
                        flag.store(true, Ordering::Relaxed);
                    }
                    port = None;
                    periodic = None;
                    queue = None;
                    file = None;
                    let _ = evt_tx.try_send(Event::Periodic(false));

                    match open_serial(&settings, lang) {
                        Ok(p) => match p.try_clone() {
                            Ok(clone) => {
                                let flag = Arc::new(AtomicBool::new(false));
                                let tx2 = evt_tx.clone();
                                let flag2 = Arc::clone(&flag);
                                std::thread::spawn(move || reader_loop(clone, tx2, flag2, lang));
                                reader_stop = Some(flag);
                                port = Some(p);
                                let _ = evt_tx
                                    .try_send(Event::Opened { port_name: settings.port_name });
                            }
                            Err(e) => {
                                let s = lang.strings();
                                let _ = evt_tx.try_send(Event::Error(s.fill(
                                    s.open_failed_fmt,
                                    &[("e", e.to_string())],
                                )));
                            }
                        },
                        Err(e) => {
                            let s = lang.strings();
                            let _ = evt_tx.try_send(Event::Error(s.fill(
                                s.open_failed_fmt,
                                &[("e", e.to_string())],
                            )));
                        }
                    }
                }
                Command::SetLanguage(l) => lang = l,
                Command::Close => {
                    if let Some(flag) = reader_stop.take() {
                        flag.store(true, Ordering::Relaxed);
                    }
                    port = None;
                    periodic = None;
                    queue = None;
                    file = None;
                    let _ = evt_tx.try_send(Event::Periodic(false));
                    let _ = evt_tx.try_send(Event::QueueStopped);
                    let _ = evt_tx.try_send(Event::FileStopped);
                    let _ = evt_tx.try_send(Event::Closed);
                }
                Command::Write(bytes) => {
                    let _ = write_all(&mut port, &evt_tx, &bytes, lang);
                }
                Command::StartPeriodic { bytes, interval_ms } => {
                    periodic = Some(PeriodicState {
                        bytes,
                        interval_ms: interval_ms.max(10),
                        next: Instant::now(),
                    });
                    let _ = evt_tx.try_send(Event::Periodic(true));
                }
                Command::StopPeriodic => {
                    periodic = None;
                    let _ = evt_tx.try_send(Event::Periodic(false));
                }
                Command::StartQueue { name, items } => {
                    if items.is_empty() {
                        let _ =
                            evt_tx.try_send(Event::Error(lang.strings().queue_empty.to_string()));
                    } else {
                        queue = Some(QueueState {
                            items,
                            index: 0,
                            wait_until: None,
                        });
                        periodic = None;
                        file = None;
                        let _ = evt_tx.try_send(Event::Periodic(false));
                        let _ = evt_tx.try_send(Event::FileStopped);
                        let total = queue.as_ref().map(|q| q.items.len()).unwrap_or(0);
                        let _ = evt_tx.try_send(Event::QueueStarted { name, total });
                    }
                }
                Command::StopQueue => {
                    queue = None;
                    let _ = evt_tx.try_send(Event::QueueStopped);
                }
                Command::StartFile {
                    path,
                    mode,
                    line_ending,
                    line_interval_ms,
                } => match FileState::new(path, mode, line_ending, line_interval_ms, lang) {
                    Ok(fs) => {
                        file = Some(fs);
                        periodic = None;
                        queue = None;
                        let _ = evt_tx.try_send(Event::Periodic(false));
                        let _ = evt_tx.try_send(Event::QueueStopped);
                        let f = file.as_ref().expect("file just set");
                        let _ = evt_tx.try_send(Event::FileStarted {
                            name: f.name.clone(),
                            total: f.total as u64,
                        });
                    }
                    Err(e) => {
                        let s = lang.strings();
                        let _ = evt_tx.try_send(Event::Error(s.fill(
                            s.file_read_failed_fmt,
                            &[("e", e.to_string())],
                        )));
                    }
                },
                Command::StopFile => {
                    file = None;
                    let _ = evt_tx.try_send(Event::FileStopped);
                }
            }
        }

        // 周期发送
        if let Some(p) = periodic.as_mut() {
            let now = Instant::now();
            if now >= p.next {
                p.next = now + Duration::from_millis(u64::from(p.interval_ms));
                if port.is_some() {
                    let _ = write_all(&mut port, &evt_tx, &p.bytes, lang);
                }
            }
        }

        // 队列发送
        tick_queue(&mut queue, &mut port, &evt_tx, lang);
        // 文件发送
        tick_file(&mut file, &mut port, &evt_tx, lang);

        // 节流：有活跃任务时 2ms 轮询，空闲时 20ms
        if port.is_some() || queue.is_some() || file.is_some() || periodic.is_some() {
            std::thread::sleep(Duration::from_millis(2));
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn open_serial(
    settings: &PortSettings,
    lang: Language,
) -> Result<Box<dyn serialport::SerialPort>, String> {
    serialport::new(&settings.port_name, settings.baud_rate)
        .data_bits(settings.data_bits.to_serial())
        .stop_bits(settings.stop_bits.to_serial(lang)?)
        .parity(settings.parity.to_serial(lang)?)
        .flow_control(settings.flow_control.to_serial())
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| e.to_string())
}

fn reader_loop(
    mut port: Box<dyn serialport::SerialPort>,
    evt_tx: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    lang: Language,
) {
    let mut buf = [0u8; 4096];
    const READ_BATCH_MAX: usize = 64 * 1024;

    while !stop.load(Ordering::Relaxed) {
        match port.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                let mut data = Vec::with_capacity(n + 4096);
                data.extend_from_slice(&buf[..n]);
                // 尽力聚合一次循环内的剩余数据，减少事件数量
                while data.len() < READ_BATCH_MAX {
                    match port.read(&mut buf) {
                        Ok(0) => break,
                        Ok(m) => data.extend_from_slice(&buf[..m]),
                        Err(_) => break,
                    }
                }
                // 阻塞投递：通道满时等待 UI 消费，绝不丢弃接收数据（背压）。
                // 数据会暂存在串口驱动缓冲区，UI 恢复后继续读取。
                if evt_tx.send(Event::Data(data)).is_err() {
                    return; // 通道已断开
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => {
                let s = lang.strings();
                let _ = evt_tx.try_send(Event::Error(s.fill(
                    s.port_read_failed_fmt,
                    &[("e", e.to_string())],
                )));
                let _ = evt_tx.try_send(Event::Disconnected);
                break;
            }
        }
    }
}

/// 写入串口；失败时发送错误事件，成功时发送 Tx 事件（用于统计与日志）。
fn write_all(
    port: &mut Option<Box<dyn serialport::SerialPort>>,
    evt_tx: &SyncSender<Event>,
    bytes: &[u8],
    lang: Language,
) -> bool {
    let Some(p) = port.as_mut() else {
        let _ = evt_tx.try_send(Event::Error(lang.strings().port_not_open.to_string()));
        return false;
    };
    write_all_dyn(&mut **p, evt_tx, bytes, lang)
}

/// 对任意 `std::io::Write` 目标执行完整写入（用于串口写入与单元测试）。
fn write_all_dyn(
    writer: &mut dyn std::io::Write,
    evt_tx: &SyncSender<Event>,
    bytes: &[u8],
    lang: Language,
) -> bool {
    // 瞬时无进展（输出缓冲满、驱动繁忙、流控暂停）时重试，避免中途误判失败。
    // 长时间（约 30 秒）无任何字节写入才放弃，防止死等。
    const STALL_SLEEP: Duration = Duration::from_millis(10);
    const STALL_LIMIT: u32 = 3000;
    let mut written = 0usize;
    let mut stall = 0u32;
    while written < bytes.len() {
        match writer.write(&bytes[written..]) {
            Ok(0) => {
                // Windows 上每个 WriteFile 有 50ms 总超时，输出缓冲满时会返回
                // 0 字节（部分写入）；稍作等待后重试，绝不在此时中止。
                stall += 1;
                if stall >= STALL_LIMIT {
                    let _ = evt_tx
                        .try_send(Event::Error(lang.strings().write_stalled.to_string()));
                    return false;
                }
                std::thread::sleep(STALL_SLEEP);
            }
            Ok(n) => {
                written += n;
                stall = 0;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock
                || e.kind() == io::ErrorKind::TimedOut =>
            {
                stall += 1;
                if stall >= STALL_LIMIT {
                    let _ = evt_tx
                        .try_send(Event::Error(lang.strings().write_stalled.to_string()));
                    return false;
                }
                std::thread::sleep(STALL_SLEEP);
            }
            Err(e) => {
                let s = lang.strings();
                let _ = evt_tx.try_send(Event::Error(s.fill(
                    s.write_failed_fmt,
                    &[("e", e.to_string())],
                )));
                return false;
            }
        }
    }
    let _ = writer.flush();
    let _ = evt_tx.try_send(Event::Tx(bytes.to_vec()));
    true
}

fn tick_queue(
    queue: &mut Option<QueueState>,
    port: &mut Option<Box<dyn serialport::SerialPort>>,
    evt_tx: &SyncSender<Event>,
    lang: Language,
) {
    let Some(state) = queue else {
        return;
    };

    if let Some(wait) = state.wait_until {
        if Instant::now() < wait {
            return;
        }
        state.wait_until = None;
    }

    if state.index >= state.items.len() {
        *queue = None;
        let _ = evt_tx.try_send(Event::QueueDone);
        return;
    }

    let item = &state.items[state.index];
    let delay = item.delay_ms;
    if !write_all(port, evt_tx, &item.bytes, lang) {
        *queue = None;
        let _ = evt_tx.try_send(Event::QueueStopped);
        return;
    }
    state.index += 1;
    let _ = evt_tx.try_send(Event::QueueProgress {
        current: state.index,
        total: state.items.len(),
    });

    if state.index >= state.items.len() {
        *queue = None;
        let _ = evt_tx.try_send(Event::QueueDone);
    } else if delay > 0 {
        state.wait_until = Some(Instant::now() + Duration::from_millis(u64::from(delay)));
    }
}

fn tick_file(
    file: &mut Option<FileState>,
    port: &mut Option<Box<dyn serialport::SerialPort>>,
    evt_tx: &SyncSender<Event>,
    lang: Language,
) {
    let Some(state) = file else {
        return;
    };

    if let Some(wait) = state.wait_until {
        if Instant::now() < wait {
            return;
        }
        state.wait_until = None;
    }

    match state.mode {
        FileSendMode::WholeFile => {
            if state.pos >= state.total {
                *file = None;
                let _ = evt_tx.try_send(Event::FileDone);
                return;
            }
            let end = (state.pos + 4096).min(state.total);
            if !write_all(port, evt_tx, &state.data[state.pos..end], lang) {
                *file = None;
                let _ = evt_tx.try_send(Event::FileStopped);
                return;
            }
            state.pos = end;
            report_file_progress(state, evt_tx);
            if state.pos >= state.total {
                *file = None;
                let _ = evt_tx.try_send(Event::FileDone);
            }
        }
        FileSendMode::LineByLine => {
            if state.line_idx >= state.lines.len() {
                *file = None;
                let _ = evt_tx.try_send(Event::FileDone);
                return;
            }
            let (start, end) = state.lines[state.line_idx];
            let mut line = state.data[start..end].to_vec();
            line.extend_from_slice(&state.line_ending);
            if !write_all(port, evt_tx, &line, lang) {
                *file = None;
                let _ = evt_tx.try_send(Event::FileStopped);
                return;
            }
            state.line_idx += 1;
            state.pos = end;
            report_file_progress(state, evt_tx);
            if state.line_idx >= state.lines.len() {
                *file = None;
                let _ = evt_tx.try_send(Event::FileDone);
            } else if state.line_interval_ms > 0 {
                state.wait_until =
                    Some(Instant::now() + Duration::from_millis(u64::from(state.line_interval_ms)));
            }
        }
    }
}

/// 节流上报文件发送进度：至少每 50ms 或每 64KB 一次，避免洪泛事件通道挤掉接收数据。
fn report_file_progress(state: &mut FileState, evt_tx: &SyncSender<Event>) {
    let now = Instant::now();
    let advanced = state.pos.saturating_sub(state.last_report_pos);
    if now.duration_since(state.last_report).as_millis() >= 50 || advanced >= 64 * 1024 {
        let _ = evt_tx.try_send(Event::FileProgress {
            sent: state.pos as u64,
            total: state.total as u64,
        });
        state.last_report = now;
        state.last_report_pos = state.pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前 N 次 write 返回 0（模拟 Windows 输出缓冲满），随后正常写入。
    struct FlakyWriter {
        zero_times: u32,
        written: Vec<u8>,
    }

    impl io::Write for FlakyWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.zero_times > 0 {
                self.zero_times -= 1;
                return Ok(0);
            }
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_all_retries_on_zero_progress() {
        let (tx, rx) = sync_channel(8);
        let mut w = FlakyWriter {
            zero_times: 3,
            written: Vec::new(),
        };
        let data = b"hello serial world";
        assert!(write_all_dyn(&mut w, &tx, data, Language::Chinese));
        assert_eq!(w.written, data);
        assert!(matches!(rx.try_recv(), Ok(Event::Tx(b)) if b == data));
    }

    /// 前 N 次 write 返回 TimedOut（模拟写超时），随后正常写入。
    struct TimeoutWriter {
        errors: u32,
        written: Vec<u8>,
    }

    impl io::Write for TimeoutWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.errors > 0 {
                self.errors -= 1;
                return Err(io::Error::new(io::ErrorKind::TimedOut, "write timeout"));
            }
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_all_retries_on_timeout() {
        let (tx, _rx) = sync_channel(8);
        let mut w = TimeoutWriter {
            errors: 2,
            written: Vec::new(),
        };
        let data = b"abc";
        assert!(write_all_dyn(&mut w, &tx, data, Language::Chinese));
        assert_eq!(w.written, data);
    }
}
