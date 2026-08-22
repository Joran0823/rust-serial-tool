//! 应用主状态与事件循环。

use crate::config::{Config, TextEncoding};
use crate::serial::{Event, SerialSession};
use crate::ui::data::DisplaySeg;
use crate::ui::theme;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SerialApp {
    pub session: SerialSession,
    pub config: Config,
    pub last_save: Instant,

    // 端口列表：(显示名, 端口名)，显示名形如 "USB-SERIAL CH340(COM3)"
    pub port_list: Vec<(String, String)>,
    pub auto_refresh_last: Instant,

    // 连接
    pub session_connected: bool,
    pub connected_port: String,
    pub baud_input: String,

    // 数据展示（接收 + 发送合并，RX/TX 标记）
    pub data_display: Vec<u8>,
    /// 数据段：标记文本 + 原始字节范围（原始字节流连续，标记不打断多字节字符）
    pub display_segments: Vec<DisplaySeg>,
    pub display_cache: String,
    pub display_dirty: bool,
    /// 已解码进 display_cache 的段数
    pub display_segments_decoded: usize,
    /// 缓存需要整体重建（编码/显示模式变化或缓冲区被裁剪）
    pub display_cache_invalid: bool,
    /// 文本模式增量解码器（保留跨批次的多字节字符）
    pub display_decoder: Option<encoding_rs::Decoder>,
    pub display_decoder_enc: Option<TextEncoding>,
    pub display_dropped: u64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub paused: bool,

    // 日志
    pub log_on: bool,
    pub log_path: Option<PathBuf>,
    pub log_file: Option<std::io::BufWriter<std::fs::File>>,

    // 发送
    pub send_input: String,
    pub send_history: Vec<String>,
    pub periodic_enabled: bool,
    pub pending_file: Option<PathBuf>,
    pub file_send_active: bool,
    pub file_progress: Option<(u64, u64)>,

    // 队列
    pub queue_sending: bool,
    pub queue_progress: Option<(usize, usize)>,

    // 状态
    pub status: String,
    pub status_error: bool,
    pub alert: Option<String>,
    /// 首次启动是否已做过窗口屏幕内钳制
    pub viewport_clamped: bool,
}

impl SerialApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx);
        let config = Config::load();
        let baud_input = config.port.baud_rate.to_string();

        let mut app = Self {
            session: SerialSession::spawn(),
            last_save: Instant::now(),
            config,
            port_list: Vec::new(),
            auto_refresh_last: Instant::now(),
            session_connected: false,
            connected_port: String::new(),
            baud_input,
            data_display: Vec::new(),
            display_segments: Vec::new(),
            display_cache: String::new(),
            display_dirty: false,
            display_segments_decoded: 0,
            display_cache_invalid: false,
            display_decoder: None,
            display_decoder_enc: None,
            display_dropped: 0,
            rx_total: 0,
            tx_total: 0,
            paused: false,
            log_on: false,
            log_path: None,
            log_file: None,
            send_input: String::new(),
            send_history: Vec::new(),
            periodic_enabled: false,
            pending_file: None,
            file_send_active: false,
            file_progress: None,
            queue_sending: false,
            queue_progress: None,
            status: "就绪".to_string(),
            status_error: false,
            alert: None,
            viewport_clamped: false,
        };
        app.refresh_ports();
        app
    }

    pub fn set_status(&mut self, msg: String, is_error: bool) {
        self.status = msg;
        self.status_error = is_error;
    }

    /// 处理事件队列，每帧最多处理 64 条，避免单帧解码过量数据导致卡顿。
    /// 返回是否仍可能有积压事件（调用方应请求继续重绘）。
    fn drain_events(&mut self) -> bool {
        let mut hit_cap = false;
        for i in 0..64 {
            match self.session.try_recv_event() {
                Some(ev) => self.handle_event(ev),
                None => break,
            }
            if i == 63 {
                hit_cap = true;
            }
        }
        hit_cap
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Opened { port_name } => {
                self.session_connected = true;
                self.connected_port = port_name.clone();
                self.set_status(format!("已连接 {port_name}"), false);
            }
            Event::Closed => {
                self.session_connected = false;
                self.connected_port.clear();
                self.set_status("已断开".to_string(), false);
            }
            Event::Disconnected => {
                self.session_connected = false;
                self.connected_port.clear();
                self.set_status("串口已断开，请检查设备连接".to_string(), true);
                self.refresh_ports();
            }
            Event::Error(msg) => self.set_status(msg, true),
            Event::Data(bytes) => {
                self.rx_total += bytes.len() as u64;
                self.log_write_rx(&bytes);
                self.append_rx(&bytes);
            }
            Event::Tx(bytes) => {
                self.tx_total += bytes.len() as u64;
                if self.config.show_sent_data {
                    self.append_tx(&bytes);
                }
            }
            Event::Periodic(on) => self.periodic_enabled = on,
            Event::QueueStarted { name, total } => {
                self.queue_sending = true;
                self.queue_progress = Some((0, total));
                self.set_status(format!("队列「{name}」开始发送，共 {total} 条"), false);
            }
            Event::QueueProgress { current, total } => {
                self.queue_progress = Some((current, total));
            }
            Event::QueueDone => {
                self.queue_sending = false;
                self.queue_progress = None;
                self.set_status("队列发送完成".to_string(), false);
            }
            Event::QueueStopped => {
                self.queue_sending = false;
                self.queue_progress = None;
                self.set_status("队列发送已停止".to_string(), false);
            }
            Event::FileStarted { name, total } => {
                self.file_send_active = true;
                self.file_progress = Some((0, total));
                self.set_status(format!("开始发送文件 {name}（{total} 字节）"), false);
            }
            Event::FileProgress { sent, total } => {
                self.file_progress = Some((sent, total));
            }
            Event::FileDone => {
                self.file_send_active = false;
                self.file_progress = None;
                self.set_status("文件发送完成".to_string(), false);
            }
            Event::FileStopped => {
                self.file_send_active = false;
                self.file_progress = None;
                self.set_status("文件发送已取消".to_string(), false);
            }
        }
    }
}

impl eframe::App for SerialApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 首次启动：若窗口超出屏幕可视区，缩小并移回屏幕内（防止底部布局/状态栏被截断）
        if !self.viewport_clamped {
            let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
                return;
            };
            self.viewport_clamped = true;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, monitor);
            let win = ctx.viewport_rect();
            let margin = 20.0;
            let mut need = false;
            let mut size = win.size();
            let mut pos = win.min;
            if win.max.x > screen.max.x - margin {
                size.x = (screen.max.x - margin - pos.x).max(320.0);
                need = true;
            }
            if win.max.y > screen.max.y - margin {
                size.y = (screen.max.y - margin - pos.y).max(360.0);
                need = true;
            }
            if pos.x < screen.min.x {
                pos.x = screen.min.x;
                need = true;
            }
            if pos.y < screen.min.y {
                pos.y = screen.min.y;
                need = true;
            }
            if need {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            }
        }

        if self.drain_events() {
            ctx.request_repaint();
        }

        if self.last_save.elapsed() > Duration::from_secs(3) {
            self.config.save();
            self.last_save = Instant::now();
        }
        if self.config.auto_refresh_ports
            && self.auto_refresh_last.elapsed() > Duration::from_secs(5)
        {
            self.auto_refresh_last = Instant::now();
            self.refresh_ports();
        }

        // 连接期间定期轮询事件通道：应用空闲时不会自动重绘，
        // 若只依赖 display_dirty 触发重绘，接收数据会滞留在通道中不上屏。
        if self.session_connected {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        if self.display_dirty
            || self.queue_sending
            || self.file_send_active
            || self.periodic_enabled
        {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 布局1：串口配置（顶部通栏，内容横向分布、高度固定、宽度自适应）
        egui::Panel::top("serial_config")
            .exact_size(48.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                self.config_panel(ui);
            });

        // 底部状态栏
        egui::Panel::bottom("status_bar")
            .exact_size(24.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::STATUS_BG)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                self.status_bar(ui);
            });

        // 主布局：5 个 layout 纵向堆叠，每个横向填满 parent、内边距统一（8px），
        // 布局之间以分割线分隔；仅布局5-子2（队列条目）支持纵向滚动
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::PANEL))
            .show(ui, |ui| {
                const PAD: f32 = 8.0; // 统一内边距
                const SEP: f32 = 8.0; // 分割线高度
                const GAP: f32 = 8.0;
                let total = ui.available_height();
                let settings_h = 34.0;     // 布局2-子1：设置行（容纳 32px 按钮）
                let send_input_h = 96.0;   // 布局3-子1：发送文本框
                let send_buttons_h = 44.0; // 布局3-子2：按钮行 + 运行状态
                let file_h = 40.0;         // 布局4：文件发送行
                let queue_h = 170.0;       // 布局5：队列
                let fixed = settings_h
                    + GAP
                    + send_input_h
                    + 6.0
                    + send_buttons_h
                    + file_h
                    + queue_h
                    + PAD * 8.0
                    + SEP * 4.0;
                let rx_h = (total - fixed).max(40.0);

                // 布局1 与 布局2 之间的分割线
                ui.separator();

                // 布局2（纵向）：子1 设置行 + 子2 接收区
                padded(ui, settings_h + GAP + rx_h + PAD * 2.0, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), settings_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.receive_settings_row(ui),
                    );
                    ui.add_space(GAP);
                    self.receive_area(ui);
                });
                ui.separator();

                // 布局3（纵向）：子1 发送文本框 + 子2 按钮行
                padded(ui, send_input_h + 6.0 + send_buttons_h + PAD * 2.0, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), send_input_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.send_input_box(ui),
                    );
                    ui.add_space(6.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), send_buttons_h),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.send_buttons_row(ui),
                    );
                });
                ui.separator();

                // 布局4（横向、高度固定、宽度自适应）：文件发送
                padded(ui, file_h + PAD * 2.0, |ui| self.file_panel(ui));
                ui.separator();

                // 布局5（纵向，子布局无边框）：队列（仅其条目列表支持纵向滚动）
                padded(ui, queue_h + PAD * 2.0, |ui| self.queue_panel(ui));
            });

        // 警告弹窗（如队列全部为空）
        if let Some(msg) = self.alert.clone() {
            egui::Window::new("提示")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(msg);
                    if ui.button("确定").clicked() {
                        self.alert = None;
                    }
                });
        }
    }

    fn on_exit(&mut self) {
        self.config.save();
    }
}

impl Drop for SerialApp {
    fn drop(&mut self) {
        self.config.save();
    }
}

/// 统一内边距（8px）的分区容器，用于布局1~5，保证视觉一致。
fn padded<R>(
    ui: &mut egui::Ui,
    height: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| egui::Frame::new().inner_margin(egui::Margin::same(8)).show(ui, add_contents).inner,
    )
    .inner
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansCJKsc-Regular.otf"
        ))),
    );
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "cjk".to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.push("cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}
