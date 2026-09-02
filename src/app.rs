//! 应用主状态与事件循环。

use crate::config::{Config, TextEncoding};
use crate::i18n::{Language, Strings};
use crate::serial::{Command, Event, SerialSession};
use crate::ui::receive::DisplaySeg;
use crate::ui::theme;
use eframe::egui;
use egui_dock::{DockArea, DockState, Node, NodeIndex, NodePath, Style, TabViewer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// “发送文件”面板正文高度：33px 控件行 + 上下对称留白。
/// dock 滚动条已关闭（`scroll_bars` 返回 false），不再受 ScrollArea
/// 64px 最小内容高度限制，高度可收紧，把空间让给下方的队列。
const FILE_BODY_H: f32 = 54.0;

/// 队列面板正文最小高度：第一行控件（33px）+ 一个队列条目（30px）+ 余量。
const QUEUE_BODY_MIN: f32 = 76.0;

/// 可停靠面板：配置 / 接收区 / 发送区 / 文件发送 / 队列。
///
/// 每个面板在 [`DockState`] 中是一个 tab，支持拖动、拆分、关闭（收起）与恢复。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiPanel {
    Config,
    Receive,
    Send,
    File,
    Queue,
}

impl UiPanel {
    /// 全部面板，顺序即默认 tab 顺序。
    pub const ALL: [UiPanel; 5] = [
        UiPanel::Config,
        UiPanel::Receive,
        UiPanel::Send,
        UiPanel::File,
        UiPanel::Queue,
    ];

    /// 当前语言下的面板标题。
    pub fn title(self, s: &Strings) -> &'static str {
        match self {
            UiPanel::Config => s.port_config,
            UiPanel::Receive => s.receive_area,
            UiPanel::Send => s.send_area,
            UiPanel::File => s.send_file,
            UiPanel::Queue => s.queue_area,
        }
    }
}

/// 默认停靠布局：顶部是配置（矮条），下方沿用原有纵向堆叠的观感
/// （接收区在上，发送/文件/队列在下），每个区域独立成块，
/// 可拖动、拆分、关闭与恢复。
fn default_dock_state() -> DockState<UiPanel> {
    let mut dock = DockState::new(vec![UiPanel::Config]);
    let root = NodeIndex::root();
    let [_, receive] = dock
        .main_surface_mut()
        .split_below(root, 0.11, vec![UiPanel::Receive]);
    let [_, send] = dock
        .main_surface_mut()
        .split_below(receive, 0.49, vec![UiPanel::Send]);
    let [_, file] = dock
        .main_surface_mut()
        .split_below(send, 0.40, vec![UiPanel::File]);
    let [_, _queue] = dock
        .main_surface_mut()
        .split_below(file, 0.19, vec![UiPanel::Queue]);
    dock
}

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

    // 终端模式（接收区）：原始字节缓冲 + 解码文本 + 输入行
    pub terminal_buffer: Vec<u8>,
    pub terminal_text: String,
    pub terminal_decoder: Option<encoding_rs::Decoder>,
    pub terminal_decoder_enc: Option<TextEncoding>,
    /// 终端文本缓存需要整体重建（编码/显示模式变化或缓冲被裁剪）
    pub terminal_cache_invalid: bool,
    /// 已解码进 terminal_text 的缓冲字节数
    pub terminal_decoded_len: usize,
    /// 终端缓冲中各批数据的起始字节偏移与时间戳标记
    /// （「显示时间戳」开启时终端同样显示）
    pub terminal_markers: Vec<(usize, String)>,
    /// 已插入 terminal_text 的标记数（重建时从 0 重新回放）
    pub terminal_markers_inserted: usize,
    /// 时间戳标记在 terminal_text 中的字节范围（用于绿色渲染）
    pub terminal_marker_spans: Vec<(usize, usize)>,
    /// shell 提示符时间戳（显示时间戳 + 回车发送模式下，输入行前缀）
    pub terminal_prompt: String,
    pub terminal_input: String,
    /// 待发送输入行中的光标位置（字符下标）
    pub terminal_cursor: usize,

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
    /// 配置面板正文（不含 tab 栏）内容高度（px）。
    ///
    /// Config 面板在 dock 中关闭了 ScrollArea 的 64px 最小高度下限，
    /// 面板高度完全由 split fraction 决定；`config_panel` 每帧写入
    /// “内容实测高 + 2×顶部实际内边距”，下一帧据此反推 fraction，
    /// 实现“自动高度 = 仅容下内部控件且上下留白一致”。
    pub config_panel_body_h: f32,
    /// 停靠布局状态：四个区域以 tab 形式停靠，可拖动/拆分/关闭（收起）与恢复。
    /// 用 Option 包一层，便于每帧取出后与 `SerialApp` 的其余部分解耦借用。
    pub dock_state: Option<DockState<UiPanel>>,
}

impl SerialApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let config = Config::load();
        let system_dark = cc.egui_ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark);
        let is_dark = config.theme.is_dark(system_dark);
        theme::apply_theme(&cc.egui_ctx, is_dark);
        let s = config.language.strings();
        let baud_input = config.port.baud_rate.to_string();

        let mut app = Self {
            session: SerialSession::spawn(config.language),
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
            terminal_buffer: Vec::new(),
            terminal_text: String::new(),
            terminal_decoder: None,
            terminal_decoder_enc: None,
            terminal_cache_invalid: false,
            terminal_decoded_len: 0,
            terminal_markers: Vec::new(),
            terminal_markers_inserted: 0,
            terminal_marker_spans: Vec::new(),
            terminal_prompt: String::new(),
            terminal_input: String::new(),
            terminal_cursor: 0,
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
            status: s.status_ready.to_string(),
            status_error: false,
            alert: None,
            viewport_clamped: false,
            config_panel_body_h: 44.0, // 首帧无实测值：按按钮高 28 + 上下边距估算
            dock_state: Some(default_dock_state()),
        };
        app.refresh_ports();
        app
    }

    /// 当前语言的全部界面文案。
    pub fn t(&self) -> &'static Strings {
        self.config.language.strings()
    }

    /// 切换界面语言：立即生效，并同步给后台会话线程（用于错误消息）。
    pub fn set_language(&mut self, lang: Language) {
        if self.config.language == lang {
            return;
        }
        self.config.language = lang;
        self.session.send(Command::SetLanguage(lang));
    }

    pub fn set_status(&mut self, msg: String, is_error: bool) {
        self.status = msg;
        self.status_error = is_error;
    }

    /// 渲染指定停靠面板的正文内容。
    pub fn panel_ui(&mut self, ui: &mut egui::Ui, panel: UiPanel) {
        match panel {
            UiPanel::Config => {
                // 配置区：内容横向流式排布，自动宽度按需换行。
                // 面板自身高度由 dock 布局的 split fraction 控制，
                // config_panel 每帧按实测内容高度写入 config_panel_body_h。
                self.config_panel(ui);
            }
            UiPanel::Receive => {
                // 接收区：控件行（流式换行）+ 分隔线 + 带滚动条的接收文本框
                ui.vertical(|ui| {
                    self.receive_settings_row(ui);
                    ui.separator();
                    if self.config.terminal_mode {
                        self.terminal_area(ui);
                    } else {
                        self.receive_area(ui);
                    }
                });
            }
            UiPanel::Send => {
                // 输入框（内部滚动）+ 底部按钮行：内容适配面板高度，
                // 顶部/底部可视留白一致（按实测顶部内边距预留同等底部空间）。
                let top_inset = (ui.cursor().top() - ui.clip_rect().top()).max(0.0);
                let btn_h = 32.0;
                let gap = 6.0;
                // allocate_ui_with_layout 的光标推进会额外带上 top_inset，
                // 因此目标底部留白按 2×top_inset 预留才能让按钮恰好贴到对称位置
                let input_h = (ui.clip_rect().bottom()
                    - 2.0 * top_inset
                    - ui.cursor().top()
                    - gap
                    - btn_h)
                    .max(24.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), input_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.send_input_box(ui),
                );
                ui.add_space(gap);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), btn_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.send_buttons_row(ui),
                );
            }
            UiPanel::File => {
                // 文件面板正文固定为 FILE_BODY_H（33px 控件行 + 上下留白），
                // 内容行垂直居中，避免控件贴顶。
                let pad = ((ui.available_height() - 33.0) / 2.0).max(0.0);
                ui.add_space(pad);
                self.file_panel(ui);
            }
            UiPanel::Queue => self.queue_panel(ui),
        }
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
                self.set_status(
                    self.t()
                        .fill(self.t().status_connected_fmt, &[("port", port_name)]),
                    false,
                );
            }
            Event::Closed => {
                self.session_connected = false;
                self.connected_port.clear();
                self.set_status(self.t().status_disconnected.to_string(), false);
            }
            Event::Disconnected => {
                self.session_connected = false;
                self.connected_port.clear();
                self.set_status(self.t().status_disconnected_error.to_string(), true);
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
                // 本地回显：非终端模式下把发送数据显示到展示区
                if self.config.terminal_auto_echo {
                    self.append_tx(&bytes);
                }
            }
            Event::Periodic(on) => self.periodic_enabled = on,
            Event::QueueStarted { name, total } => {
                self.queue_sending = true;
                self.queue_progress = Some((0, total));
                self.set_status(
                    self.t().fill(
                        self.t().queue_started_fmt,
                        &[("name", name), ("total", total.to_string())],
                    ),
                    false,
                );
            }
            Event::QueueProgress { current, total } => {
                self.queue_progress = Some((current, total));
            }
            Event::QueueDone => {
                self.queue_sending = false;
                self.queue_progress = None;
                self.set_status(self.t().queue_done.to_string(), false);
            }
            Event::QueueStopped => {
                self.queue_sending = false;
                self.queue_progress = None;
                self.set_status(self.t().queue_stopped.to_string(), false);
            }
            Event::FileStarted { name, total } => {
                self.file_send_active = true;
                self.file_progress = Some((0, total));
                self.set_status(
                    self.t().fill(
                        self.t().file_started_fmt,
                        &[("name", name), ("total", total.to_string())],
                    ),
                    false,
                );
            }
            Event::FileProgress { sent, total } => {
                self.file_progress = Some((sent, total));
            }
            Event::FileDone => {
                self.file_send_active = false;
                self.file_progress = None;
                self.set_status(self.t().file_done.to_string(), false);
            }
            Event::FileStopped => {
                self.file_send_active = false;
                self.file_progress = None;
                self.set_status(self.t().file_cancelled.to_string(), false);
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
        // 底部状态栏
        egui::Panel::bottom("status_bar")
            .exact_size(24.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::status_bg())
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                self.status_bar(ui);
            });

        // 主布局：四个区域以停靠 tab 呈现，可拖动、拆分、关闭（收起）与恢复。
        // 先取出 dock_state，使 viewer 可以独占借用 self，渲染结束后再放回。
        let mut dock = self.dock_state.take().expect("dock_state 应始终存在");
        // 上一帧实测的配置面板正文高度（viewer 借用 self 前先取出）
        let config_body_h = self.config_panel_body_h;
        let open_panels = current_open_panels(&dock);
        let mut viewer = DockViewer {
            app: self,
            open_panels,
            pending_restore: None,
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::panel()))
            .show(ui, |ui| {
                if dock.main_surface().is_empty() {
                    // 全部面板都已收起：显示恢复入口
                    restore_panels_ui(ui, viewer.app, &mut dock);
                } else {
                    // “发送文件”面板固定为仅容纳内容的高度，不随窗口缩放
                    let style = dock_style(ui);
                    let tab_bar_h = style.tab_bar.height;
                    fix_file_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + FILE_BODY_H,
                    );
                    // “配置”面板固定为仅容纳内容的高度（内容高 + 2×顶部内边距）
                    fix_config_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + config_body_h,
                    );
                    // “队列”面板高度至少容纳第一行 + 一个条目
                    fix_queue_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + QUEUE_BODY_MIN,
                    );
                    DockArea::new(&mut dock)
                        .style(style)
                        .show_add_buttons(false)
                        .show_add_popup(false)
                        // 面板用“折叠/展开”按钮收起布局，避免关闭后需通过 + 菜单才能找回
                        .show_close_buttons(false)
                        .show_leaf_collapse_buttons(true)
                        .show_leaf_close_all_buttons(false)
                        .show_secondary_button_hint(false)
                        .tab_context_menus(false)
                        .show_inside(ui, &mut viewer);
                    draw_collapse_icons(ui, &dock, tab_bar_h);
                }
            });
        if let Some(panel) = viewer.pending_restore.take() {
            let (panel, node) = panel;
            let restored = dock
                .main_surface_mut()
                .leaf_mut(node)
                .is_ok_and(|leaf| {
                    leaf.append_tab(panel);
                    true
                });
            if !restored {
                // 该分组已被移除（如同一帧内被关闭），退回第一个分组
                dock.main_surface_mut().push_to_first_leaf(panel);
            }
        }
        let app = viewer.app;
        app.dock_state = Some(dock);

        // 警告弹窗（如队列全部为空）
        if let Some(msg) = self.alert.clone() {
            egui::Window::new(self.t().alert_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(msg);
                    if ui.button(self.t().ok).clicked() {
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

/// DockArea 的 TabViewer：渲染期间独占持有 `SerialApp` 的可变引用，
/// 并通过快照与挂起动作实现“关闭后从 + 菜单恢复面板”。
struct DockViewer<'a> {
    app: &'a mut SerialApp,
    /// 本帧开始时各面板是否处于打开状态（按 [`UiPanel::ALL`] 顺序）。
    open_panels: [bool; 5],
    /// “+” 菜单中点击待恢复的面板及其所属分组，渲染结束后应用到 dock_state。
    pending_restore: Option<(UiPanel, NodeIndex)>,
}

impl TabViewer for DockViewer<'_> {
    type Tab = UiPanel;

    /// 每个 tab 的唯一 ID（UiPanel 已实现 Hash）。
    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title(self.app.t()).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.app.panel_ui(ui, *tab);
    }

    /// 配置面板关闭双向滚动：ScrollArea 对启用方向的维度强制
    /// `min_scrolled_size = 64px`（egui 0.36 默认），这是 dock 内面板高度的下限；
    /// 关闭后 Config 面板高度完全由 split fraction 决定，可由每帧实测内容高度
    /// 反推，实现“仅容下内部控件”。发送文本框、文件行、队列列表内部各自滚动，
    /// 因此也关闭 dock 滚动条；其余面板保留滚动。
    fn scroll_bars(&self, tab: &UiPanel) -> [bool; 2] {
        match tab {
            // 配置/发送/文件/队列关闭 dock 滚动条：内容各自适配或内部滚动
            UiPanel::Config | UiPanel::Send | UiPanel::File | UiPanel::Queue => [false, false],
            _ => [true, true],
        }
    }

    /// “+” 按钮弹出的面板列表：已打开的面板置灰，收起的面板可点击恢复。
    fn add_popup(&mut self, ui: &mut egui::Ui, path: NodePath) {
        let s = self.app.t();
        for (i, panel) in UiPanel::ALL.into_iter().enumerate() {
            if self.open_panels[i] {
                ui.add_enabled(false, egui::Button::new(panel.title(s)));
            } else if ui.button(panel.title(s)).clicked() {
                self.pending_restore = Some((panel, path.node));
                ui.close();
            }
        }
    }
}

/// 统计当前处于打开状态的面板（仅统计主 surface，窗口 surface 暂不参与恢复管理）。
fn current_open_panels(dock: &DockState<UiPanel>) -> [bool; 5] {
    let mut open = [false; 5];
    for tab in dock.main_surface().tabs() {
        match tab {
            UiPanel::Config => open[0] = true,
            UiPanel::Receive => open[1] = true,
            UiPanel::Send => open[2] = true,
            UiPanel::File => open[3] = true,
            UiPanel::Queue => open[4] = true,
        }
    }
    open
}

/// 计算某个节点在给定总高度下实际分到的高度（按各层纵向拆分的 fraction 累乘）。
/// 水平拆分不改变高度，直接继承父节点高度。
fn node_height(dock: &DockState<UiPanel>, node: NodeIndex, total_h: f32) -> f32 {
    if node == NodeIndex::root() {
        return total_h;
    }
    let parent = node.parent().expect("非根节点必有父节点");
    let parent_h = node_height(dock, parent, total_h);
    let Node::Vertical(split) = &dock.main_surface()[parent] else {
        return parent_h;
    };
    if node == parent.left() {
        parent_h * split.fraction
    } else {
        parent_h * (1.0 - split.fraction)
    }
}

/// 将“发送文件”面板固定为仅容纳内容的高度。
///
/// 仅在其独立成块（leaf 中只有 File 一个 tab）且父级是纵向拆分时生效，
/// 其余布局（合并成 tab 组、横向并排等）交给用户自由调整。
fn fix_file_panel_height(
    dock: &mut DockState<UiPanel>,
    total_h: f32,
    file_total_h: f32,
) {
    let Some((file_node, _)) = dock.main_surface().find_tab(&UiPanel::File) else {
        return;
    };
    let standalone = dock
        .main_surface()
        .leaf(file_node)
        .map_or(true, |leaf| leaf.tabs().len() != 1);
    if standalone {
        return;
    }
    let Some(parent) = file_node.parent() else {
        return;
    };
    let parent_h = node_height(dock, parent, total_h);
    match &dock.main_surface()[parent] {
        Node::Vertical(_) => {}
        _ => return,
    }
    if parent_h <= 0.0 {
        return;
    }

    // File 是父拆分的左（上）子节点时，fraction 即其占比；右（下）子节点时取补集
    let target = (file_total_h / parent_h).clamp(0.05, 0.95);
    let target = if file_node == parent.left() {
        target
    } else {
        1.0 - target
    };
    if let Node::Vertical(split) = &mut dock.main_surface_mut()[parent] {
        split.fraction = target;
    }
}

/// 将“配置”面板固定为仅容纳内容的高度（不含 tab 栏）。
///
/// 与 [`fix_file_panel_height`] 同机制：仅在其独立成块且父级是纵向拆分时生效，
/// 目标高度 = 实测内容高度 + 2×顶部实际内边距（上下留白一致）。
fn fix_config_panel_height(
    dock: &mut DockState<UiPanel>,
    total_h: f32,
    config_total_h: f32,
) {
    let Some((config_node, _)) = dock.main_surface().find_tab(&UiPanel::Config) else {
        return;
    };
    let standalone = dock
        .main_surface()
        .leaf(config_node)
        .map_or(true, |leaf| leaf.tabs().len() != 1);
    if standalone {
        return;
    }
    let Some(parent) = config_node.parent() else {
        return;
    };
    let parent_h = node_height(dock, parent, total_h);
    match &dock.main_surface()[parent] {
        Node::Vertical(_) => {}
        _ => return,
    }
    if parent_h <= 0.0 {
        return;
    }

    // Config 是父拆分的左（上）子节点时，fraction 即其占比；右（下）子节点时取补集
    let target = (config_total_h / parent_h).clamp(0.02, 0.95);
    let target = if config_node == parent.left() {
        target
    } else {
        1.0 - target
    };
    if let Node::Vertical(split) = &mut dock.main_surface_mut()[parent] {
        split.fraction = target;
    }
}

/// 保证“队列”面板高度不低于最小要求（第一行控件 + 一个条目）。
/// “发送文件”与“队列”在同一纵向分组内：若分组总高不足以同时放下
/// 固定高度的文件面板与最小高度的队列，则扩大分组在上级拆分中的份额。
/// 只在下限方向生效：分组已足够大时保持用户设置不动。
fn fix_queue_panel_height(
    dock: &mut DockState<UiPanel>,
    total_h: f32,
    queue_min_total_h: f32,
) {
    let Some((queue_node, _)) = dock.main_surface().find_tab(&UiPanel::Queue) else {
        return;
    };
    let standalone = dock
        .main_surface()
        .leaf(queue_node)
        .map_or(true, |leaf| leaf.tabs().len() != 1);
    if standalone {
        return;
    }
    let Some(group) = queue_node.parent() else {
        return;
    };
    match &dock.main_surface()[group] {
        Node::Vertical(_) => {}
        _ => return,
    }
    // 队列同级（另一侧）应是与它同组的“发送文件”
    let other = if queue_node == group.left() {
        group.right()
    } else {
        group.left()
    };
    let sibling_total = node_height(dock, other, total_h);
    // 预留少量余量：吸收分组内分隔条与像素取整
    let needed_group = sibling_total + queue_min_total_h + 8.0;
    let group_h = node_height(dock, group, total_h);
    if group_h >= needed_group {
        return;
    }
    let Some(grand) = group.parent() else {
        return;
    };
    let grand_h = node_height(dock, grand, total_h);
    match &dock.main_surface()[grand] {
        Node::Vertical(_) => {}
        _ => return,
    }
    if grand_h <= 0.0 {
        return;
    }
    let target = (needed_group / grand_h).clamp(0.05, 0.95);
    let target = if group == grand.left() {
        target
    } else {
        1.0 - target
    };
    if let Node::Vertical(split) = &mut dock.main_surface_mut()[grand] {
        split.fraction = target;
    }
}

/// 全部面板都已收起时的空布局：居中展示各面板按钮，点击即恢复。
fn restore_panels_ui(
    ui: &mut egui::Ui,
    app: &SerialApp,
    dock: &mut DockState<UiPanel>,
) {
    let s = app.t();
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(egui::RichText::new(s.panels_hidden).color(theme::text_soft()));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for panel in UiPanel::ALL {
                if ui
                    .add_sized([110.0, 32.0], egui::Button::new(panel.title(s)))
                    .clicked()
                {
                    dock.main_surface_mut().push_to_first_leaf(panel);
                }
            }
        });
    });
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

/// dock 主题：与全局深色主题一致的 tab 栏、分隔条与面板圆角。
fn dock_style(ui: &egui::Ui) -> egui_dock::Style {
    let mut style = Style::from_egui(ui.style());
    style.main_surface_border_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(8));
    style.main_surface_border_rounding = egui::CornerRadius::same(6);

    style.separator.width = 1.0;
    style.separator.color_idle = egui::Color32::from_white_alpha(8);
    style.separator.color_hovered = theme::BLUE_CHECK;
    style.separator.color_dragged = theme::BLUE;
    // 关键：separator.extra 是拖拽分割线时 fraction 的 clamp 下限约束
    // （默认 175px），导致任何“固定为内容高度”的面板都无法低于 ~175px——
    // DockArea 渲染时会把 fraction 强制 clamp 回来。设为 0 消除下限。
    style.separator.extra = 0.0;

    style.tab_bar.height = 30.0;
    style.tab_bar.bg_fill = theme::panel();
    style.tab_bar.inner_margin = egui::Margin::symmetric(8, 0);
    style.tab_bar.corner_radius = egui::CornerRadius::same(6);
    style.tab_bar.hline_color = egui::Color32::from_white_alpha(10);

    style.tab.spacing = 8.0;
    // tab 标题按纯文本显示：各状态透明底、无描边，仅用文字颜色区分激活态
    let label_style = |mut s: egui_dock::TabInteractionStyle,
                       color: egui::Color32|
     -> egui_dock::TabInteractionStyle {
        s.bg_fill = egui::Color32::TRANSPARENT;
        s.outline_color = egui::Color32::TRANSPARENT;
        s.text_color = color;
        s.corner_radius = egui::CornerRadius::same(theme::CORNER);
        s
    };
    style.tab.active = label_style(style.tab.active, theme::text());
    style.tab.active_with_kb_focus = label_style(style.tab.active_with_kb_focus, theme::text());
    style.tab.focused = label_style(style.tab.focused, theme::text());
    style.tab.focused_with_kb_focus = label_style(style.tab.focused_with_kb_focus, theme::text());
    style.tab.hovered = label_style(style.tab.hovered, theme::text());
    style.tab.inactive = label_style(style.tab.inactive, theme::text_soft());
    style.tab.inactive_with_kb_focus =
        label_style(style.tab.inactive_with_kb_focus, theme::text());

    style.tab.tab_body.bg_fill = theme::panel();
    style.tab.tab_body.stroke =
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(8));
    style.tab.tab_body.corner_radius = egui::CornerRadius::same(6);
    style.tab.tab_body.inner_margin = egui::Margin::same(6);

    // 收缩按钮：透明底、柔和箭头，悬停变蓝（图标形状由 egui_dock 内部绘制）
    style.buttons.collapse_tabs_bg_fill = egui::Color32::TRANSPARENT;
    style.buttons.collapse_tabs_active_color = theme::BLUE;
    style.buttons.collapse_tabs_color = theme::text_soft();
    style.buttons.collapse_tabs_border_color = egui::Color32::TRANSPARENT;

    style.overlay.selection_color =
        egui::Color32::from_rgba_unmultiplied(0x2E, 0x7C, 0xF6, 120);
    style
}

/// egui_dock 内置收缩按钮是库内部画死的三角箭头，无法通过 API 替换图形；
/// 这里在其上方覆盖绘制 “−/+” 图标，同时保留内置按钮的占位与点击逻辑。
fn draw_collapse_icons(ui: &egui::Ui, dock: &DockState<UiPanel>, tab_bar_h: f32) {
    const BTN_W: f32 = 24.0; // 与 egui_dock 内置收缩按钮宽度一致
    let hover = ui.ctx().pointer_hover_pos();
    let painter = ui.painter();
    for (path, leaf) in dock.iter_leaves() {
        if !path.surface.is_main() || leaf.tabs.is_empty() {
            continue;
        }
        let rect = egui::Rect::from_min_size(leaf.rect.min, egui::vec2(BTN_W, tab_bar_h));
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        // 先以面板色覆盖库内置三角，再绘制加减号
        painter.rect_filled(rect, egui::CornerRadius::ZERO, theme::panel());
        let color = if hover.is_some_and(|p| rect.contains(p)) {
            theme::BLUE
        } else {
            theme::text_soft()
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            if leaf.collapsed { "+" } else { "−" },
            egui::FontId::proportional(14.0),
            color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> SerialApp {
        SerialApp {
            session: SerialSession::spawn(Language::Chinese),
            config: Config::default(),
            last_save: Instant::now(),
            port_list: Vec::new(),
            auto_refresh_last: Instant::now(),
            session_connected: false,
            connected_port: String::new(),
            baud_input: String::new(),
            data_display: Vec::new(),
            display_segments: Vec::new(),
            display_cache: String::new(),
            display_dirty: false,
            display_segments_decoded: 0,
            display_cache_invalid: false,
            display_decoder: None,
            display_decoder_enc: None,
            display_dropped: 0,
            terminal_buffer: Vec::new(),
            terminal_text: String::new(),
            terminal_decoder: None,
            terminal_decoder_enc: None,
            terminal_cache_invalid: false,
            terminal_decoded_len: 0,
            terminal_markers: Vec::new(),
            terminal_markers_inserted: 0,
            terminal_marker_spans: Vec::new(),
            terminal_prompt: String::new(),
            terminal_input: String::new(),
            terminal_cursor: 0,
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
            status: String::new(),
            status_error: false,
            alert: None,
            viewport_clamped: false,
            config_panel_body_h: 44.0,
            dock_state: Some(default_dock_state()),
        }
    }

    /// 运行一帧 UI。egui 0.36 起 `FullOutput` 携带纹理增量，测试不渲染到屏幕，
    /// 必须先 `clear()`，否则丢弃时触发 epaint 的 panic 检查。
    fn run_ui(
        ctx: &egui::Context,
        input: egui::RawInput,
        f: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let mut out = ctx.run_ui(input, f);
        out.textures_delta.clear();
        out
    }

    #[test]
    fn default_dock_state_contains_all_panels() {
        let dock = default_dock_state();
        let tabs: Vec<UiPanel> = dock.main_surface().tabs().copied().collect();
        assert_eq!(tabs.len(), 5);
        for panel in UiPanel::ALL {
            assert!(tabs.contains(&panel));
        }
    }

    #[test]
    fn open_panels_snapshot_tracks_closed_tabs() {
        let mut dock = default_dock_state();
        assert!(current_open_panels(&dock).iter().all(|&open| open));

        let path = dock.main_surface().find_tab(&UiPanel::Queue).unwrap();
        dock.main_surface_mut().remove_tab(path);

        let open = current_open_panels(&dock);
        assert!(!open[4]);
        assert!(open[0] && open[1] && open[2] && open[3]);
    }

    #[test]
    fn all_closed_then_restore_creates_leaf_again() {
        let mut dock = default_dock_state();
        for panel in UiPanel::ALL {
            let path = dock.main_surface().find_tab(&panel).unwrap();
            dock.main_surface_mut().remove_tab(path);
        }
        assert!(dock.main_surface().is_empty());

        dock.main_surface_mut().push_to_first_leaf(UiPanel::Receive);
        assert_eq!(dock.main_surface().num_tabs(), 1);
    }

    #[test]
    fn file_panel_height_is_fixed_to_content() {
        let mut dock = default_dock_state();
        let total_h = 700.0;
        let tab_h = 24.0;
        fix_file_panel_height(&mut dock, total_h, tab_h + FILE_BODY_H);

        let (file_node, _) = dock.main_surface().find_tab(&UiPanel::File).unwrap();
        let h = node_height(&dock, file_node, total_h);
        assert!((h - (tab_h + FILE_BODY_H)).abs() < 0.01);
    }

    #[test]
    fn config_panel_height_is_fixed_to_content() {
        // Config 面板关闭了 ScrollArea 的 64px 最小高度下限，高度完全由
        // split fraction 决定；fix_config_panel_height 应将其收敛到
        // 「tab 栏 + 上一帧实测内容高度 + tab 正文外边距」。
        let mut dock = default_dock_state();
        let total_h = 700.0;
        let tab_h = 24.0;
        let body_h = 44.0; // 上一帧实测的 flex 内容高度（单行：按钮 28 + 上下 8 边距）
        fix_config_panel_height(&mut dock, total_h, tab_h + body_h + 8.0);

        let (config_node, _) = dock.main_surface().find_tab(&UiPanel::Config).unwrap();
        let h = node_height(&dock, config_node, total_h);
        assert!((h - (tab_h + body_h + 8.0)).abs() < 0.01);
        // 该高度应显著低于 ScrollArea 的 64px 下限：证明关闭滚动后
        // dock 面板高度不再被 ScrollArea 强制撑高。
        assert!(
            body_h < 64.0,
            "配置面板内容高度 {body_h} 应低于 ScrollArea 64px 下限，否则无需关闭滚动"
        );
    }

    /// 诊断：多帧渲染默认 dock，打印 config_panel_body_h 实测值、
    /// Config 面板实际分配高度与语言按钮矩形 y 范围。
    #[test]
    fn debug_config_panel_height_in_dock() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        let mut app = make_app();
        app.config.language = Language::Chinese;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 740.0),
            )),
            ..Default::default()
        };
        let s = Language::Chinese.strings();
        for frame in 0..5 {
            let output = run_ui(&ctx, input.clone(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut dock = app.dock_state.take().expect("dock");
                    let config_body_h = app.config_panel_body_h;
                    let open_panels = current_open_panels(&dock);
                    let mut viewer = DockViewer {
                        app: &mut app,
                        open_panels,
                        pending_restore: None,
                    };
                    let style = dock_style(ui);
                    fix_file_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + FILE_BODY_H,
                    );
                    fix_config_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + config_body_h,
                    );
                    DockArea::new(&mut dock)
                        .style(style)
                        .show_add_buttons(false)
                        .show_add_popup(false)
                        .show_close_buttons(false)
                        .show_leaf_collapse_buttons(true)
                        .show_leaf_close_all_buttons(false)
                        .show_secondary_button_hint(false)
                        .tab_context_menus(false)
                        .show_inside(ui, &mut viewer);
                    app.dock_state = Some(dock);
                });
            });
            // Config 面板实际高度
            let (config_node, _) = app
                .dock_state
                .as_ref()
                .unwrap()
                .main_surface()
                .find_tab(&UiPanel::Config)
                .unwrap();
            let panel_h = node_height(app.dock_state.as_ref().unwrap(), config_node, 700.0);
            // 语言按钮矩形 y 范围
            let mut lang_rect: Option<egui::Rect> = None;
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape
                    && ts.galley.job.text == s.language
                {
                    // 从文本反推按钮矩形（按钮高约 28，文本居中）
                    let h = ts.galley.rect.height();
                    lang_rect = Some(egui::Rect::from_min_max(
                        egui::pos2(ts.pos.x, ts.pos.y - (28.0 - h) / 2.0),
                        egui::pos2(ts.pos.x + 60.0, ts.pos.y - (28.0 - h) / 2.0 + 28.0),
                    ));
                }
            }
            println!(
                "frame={frame} body_h={:.1} panel_h={:.1} lang_rect={:?}",
                app.config_panel_body_h, panel_h, lang_rect
            );
        }
        // 1100px 窗口下配置会换行成两行：正文高度 = 内容 60 + 2×顶部内边距
        // ~10.5×2 ≈ 81，面板（tab 30 + body）对应 ~111。
        let (config_node, _) = app
            .dock_state
            .as_ref()
            .unwrap()
            .main_surface()
            .find_tab(&UiPanel::Config)
            .unwrap();
        let panel_h = node_height(app.dock_state.as_ref().unwrap(), config_node, 700.0);
        assert!(
            (app.config_panel_body_h - 81.0).abs() < 4.0,
            "配置面板内容高度异常: body_h={:.1}（两行实测应为 ~81）",
            app.config_panel_body_h
        );
        assert!(
            panel_h < 200.0,
            "Config 面板高度 {panel_h:.1} 仍过高（应收敛到 ~102）"
        );
    }

    #[test]
    fn config_panel_auto_height_fits_wrapped_rows_in_dock() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        // 窄窗口：配置必然换行到两行
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 740.0),
            )),
            ..Default::default()
        };
        for lang in [Language::Chinese, Language::English] {
            let mut app = make_app();
            app.config.language = lang;
            let s = lang.strings();
            let mut output = None;
            for _frame in 0..6 {
                output = Some(run_ui(&ctx, input.clone(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                        let config_body_h = app.config_panel_body_h;
                        let open_panels = current_open_panels(&dock);
                        let mut viewer = DockViewer {
                            app: &mut app,
                            open_panels,
                            pending_restore: None,
                        };
                        let style = dock_style(ui);
                        fix_file_panel_height(
                            &mut dock,
                            ui.available_height(),
                            style.tab_bar.height + FILE_BODY_H,
                        );
                        fix_config_panel_height(
                            &mut dock,
                            ui.available_height(),
                            style.tab_bar.height + config_body_h,
                        );
                        DockArea::new(&mut dock)
                            .style(style)
                            .show_add_buttons(false)
                            .show_add_popup(false)
                            .show_close_buttons(false)
                            .show_leaf_collapse_buttons(true)
                            .show_leaf_close_all_buttons(false)
                            .show_secondary_button_hint(false)
                            .tab_context_menus(false)
                            .show_inside(ui, &mut viewer);
                        app.dock_state = Some(dock);
                    });
                }));
            }
            // 内容高度估算应显著大于单行估计 44（至少两行）
            assert!(
                app.config_panel_body_h >= 56.0,
                "[{lang:?}] 换行后未按多行内容自适应: body_h={:.1}",
                app.config_panel_body_h
            );
            // 配置面板正文（viewport）必须容下最后一行控件：所有 label 文本
            // 都应完整落在正文区内，不得被裁剪。
            let (config_node, _) = app
                .dock_state
                .as_ref()
                .unwrap()
                .main_surface()
                .find_tab(&UiPanel::Config)
                .unwrap();
            let viewport = app
                .dock_state
                .as_ref()
                .unwrap()
                .main_surface()
                .leaf(config_node)
                .unwrap()
                .viewport;
            let out = output.expect("至少渲染一帧");
            let labels = [
                s.language,
                s.theme,
                s.open_port,
                s.auto_refresh,
                s.baud_rate,
                s.data_bits,
                s.stop_bits,
                s.parity,
                s.flow_control,
            ];
            let mut found = 0;
            for clipped in &out.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                    let text = &ts.galley.job.text;
                    if !labels.contains(&text.as_str()) {
                        continue;
                    }
                    found += 1;
                    let top = ts.pos.y;
                    let bottom = top + ts.galley.rect.height();
                    assert!(
                        top >= viewport.top() - 0.5 && bottom <= viewport.bottom() + 0.5,
                        "[{lang:?}] 配置面板控件被裁剪: {text} y 范围 {top:.1}..{bottom:.1}，正文区 {:.1}..{:.1}",
                        viewport.top(),
                        viewport.bottom()
                    );
                }
            }
            assert_eq!(found, labels.len(), "[{lang:?}] 应找到全部配置面板文本");
        }
    }

    #[test]
    fn config_panel_auto_height_fits_all_widths() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        for width in [900.0f32, 1100.0, 1280.0, 1440.0, 1600.0, 1920.0] {
            let mut app = make_app();
            app.config.language = Language::Chinese;
            let s = Language::Chinese.strings();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            };
            let mut output = None;
            for _frame in 0..4 {
                output = Some(run_ui(&ctx, input.clone(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                        let config_body_h = app.config_panel_body_h;
                        let open_panels = current_open_panels(&dock);
                        let mut viewer = DockViewer {
                            app: &mut app,
                            open_panels,
                            pending_restore: None,
                        };
                        let style = dock_style(ui);
                        fix_file_panel_height(
                            &mut dock,
                            ui.available_height(),
                            style.tab_bar.height + FILE_BODY_H,
                        );
                        fix_config_panel_height(
                            &mut dock,
                            ui.available_height(),
                            style.tab_bar.height + config_body_h,
                        );
                        DockArea::new(&mut dock)
                            .style(style)
                            .show_add_buttons(false)
                            .show_add_popup(false)
                            .show_close_buttons(false)
                            .show_leaf_collapse_buttons(true)
                            .show_leaf_close_all_buttons(false)
                            .show_secondary_button_hint(false)
                            .tab_context_menus(false)
                            .show_inside(ui, &mut viewer);
                        app.dock_state = Some(dock);
                    });
                }));
            }
            let (config_node, _) = app
                .dock_state
                .as_ref()
                .unwrap()
                .main_surface()
                .find_tab(&UiPanel::Config)
                .unwrap();
            let viewport = app
                .dock_state
                .as_ref()
                .unwrap()
                .main_surface()
                .leaf(config_node)
                .unwrap()
                .viewport;
            let out = output.expect("frame");
            let mut ys = Vec::new();
            let mut max_bottom = f32::MIN;
            let mut missing = Vec::new();
            for clipped in &out.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                    let t = &ts.galley.job.text;
                    if [
                        s.language,
                        s.theme,
                        s.open_port,
                        s.auto_refresh,
                        s.baud_rate,
                        s.data_bits,
                        s.stop_bits,
                        s.parity,
                        s.flow_control,
                    ]
                    .contains(&t.as_str())
                    {
                        ys.push((t.clone(), ts.pos.y));
                        max_bottom = max_bottom.max(ts.pos.y + ts.galley.rect.height());
                    }
                }
            }
            for label in [
                s.language,
                s.theme,
                s.open_port,
                s.auto_refresh,
                s.baud_rate,
                s.data_bits,
                s.stop_bits,
                s.parity,
                s.flow_control,
            ] {
                if !ys.iter().any(|(t, _)| t == label) {
                    missing.push(label);
                }
            }
            assert!(
                missing.is_empty(),
                "width={width} 缺失控件: {missing:?}"
            );
            assert!(
                max_bottom <= viewport.bottom() + 0.5,
                "width={width} 控件被裁剪: max_bottom={max_bottom:.1} 超出正文区 {:.1}",
                viewport.bottom()
            );
            // 面板上下留白应一致：内容顶部到正文顶 ≈ 正文底到内容底部
            let min_top = ys
                .iter()
                .map(|(_, y)| *y)
                .fold(f32::MAX, f32::min);
            let top_pad = min_top - viewport.top();
            let bottom_pad = viewport.bottom() - max_bottom;
            assert!(
                (top_pad - bottom_pad).abs() < 1.5,
                "width={width} 配置面板上下留白不一致: top={top_pad:.1} bottom={bottom_pad:.1}",
            );
        }
    }

    #[test]
    fn baud_frame_height_matches_port_combo_in_dock() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        let mut app = make_app();
        app.config.language = Language::Chinese;
        app.baud_input = "115200".to_owned();
        app.config.port.baud_rate = 115200;
        let s = Language::Chinese.strings();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 800.0),
            )),
            ..Default::default()
        };
        let output = run_ui(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                let config_body_h = app.config_panel_body_h;
                let open_panels = current_open_panels(&dock);
                let mut viewer = DockViewer {
                    app: &mut app,
                    open_panels,
                    pending_restore: None,
                };
                let style = dock_style(ui);
                fix_file_panel_height(
                    &mut dock,
                    ui.available_height(),
                    style.tab_bar.height + FILE_BODY_H,
                );
                fix_config_panel_height(
                    &mut dock,
                    ui.available_height(),
                    style.tab_bar.height + config_body_h,
                );
                DockArea::new(&mut dock)
                    .style(style)
                    .show_add_buttons(false)
                    .show_add_popup(false)
                    .show_close_buttons(false)
                    .show_leaf_collapse_buttons(true)
                    .show_leaf_close_all_buttons(false)
                    .show_secondary_button_hint(false)
                    .tab_context_menus(false)
                    .show_inside(ui, &mut viewer);
                app.dock_state = Some(dock);
            });
        });
        let rect_of = |text: &str| {
            output
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::epaint::Shape::Rect(rs)
                        if (20.0..=40.0).contains(&rs.rect.height())
                            && output.shapes.iter().any(|c2| {
                                matches!(&c2.shape, egui::epaint::Shape::Text(ts)
                                    if ts.galley.job.text == text
                                        && rs.rect.contains(egui::pos2(
                                            ts.pos.x + 2.0,
                                            ts.pos.y + 2.0,
                                        )))
                            }) =>
                    {
                        Some(rs.rect)
                    }
                    _ => None,
                })
                .max_by(|a, b| a.height().total_cmp(&b.height()))
        };
        let baud_frame = rect_of("115200").expect("应找到波特率输入框矩形");
        let port_combo = rect_of(s.port_placeholder).expect("应找到端口下拉框矩形");
        assert!(
            (baud_frame.height() - port_combo.height()).abs() < 1.0,
            "dock 中波特率输入框高度 {:.1} 与端口下拉框高度 {:.1} 不一致",
            baud_frame.height(),
            port_combo.height()
        );
    }

    #[test]
    fn file_panel_controls_centered_in_default_dock() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        let mut app = make_app();
        app.config.language = Language::Chinese;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 740.0),
            )),
            ..Default::default()
        };
        let mut tab_h = 24.0_f32;
        let output = run_ui(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                let open_panels = current_open_panels(&dock);
                let mut viewer = DockViewer {
                    app: &mut app,
                    open_panels,
                    pending_restore: None,
                };
                let style = dock_style(ui);
                tab_h = style.tab_bar.height;
                fix_file_panel_height(
                    &mut dock,
                    ui.available_height(),
                    style.tab_bar.height + FILE_BODY_H,
                );
                DockArea::new(&mut dock)
                    .style(style)
                    .show_add_buttons(false)
                    .show_add_popup(false)
                    .show_close_buttons(false)
                    .show_leaf_collapse_buttons(true)
                    .show_leaf_close_all_buttons(false)
                    .show_secondary_button_hint(false)
                    .tab_context_menus(false)
                    .show_inside(ui, &mut viewer);
                app.dock_state = Some(dock);
            });
        });
        let s = Language::Chinese.strings();

        // 从形状里找出文件面板 tab 标题、队列面板 tab 标题（各自较小的 y 值），
        // 两者之间的区域即“发送文件”面板正文（含 tab 栏下的正文区）。
        let mut file_tab_title_top = f32::MAX;
        let mut file_tab_title_h = 0.0_f32;
        let mut queue_tab_title_top = f32::MAX;
        let mut queue_tab_title_h = 0.0_f32;
        let mut file_buttons = Vec::new();
        for clipped in &output.shapes {
            if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                let text = &ts.galley.job.text;
                if text == s.send_file {
                    if ts.pos.y < file_tab_title_top {
                        file_tab_title_top = ts.pos.y;
                        file_tab_title_h = ts.galley.rect.height();
                    }
                } else if text == s.queue_area && ts.pos.y < queue_tab_title_top {
                    queue_tab_title_top = ts.pos.y;
                    queue_tab_title_h = ts.galley.rect.height();
                }
            }
        }
        // tab 标题文字在 tab 栏内垂直居中，由文字顶端与高度反推 tab 栏范围
        let file_tab_bar_top = file_tab_title_top - (tab_h - file_tab_title_h) / 2.0;
        let body_top = file_tab_bar_top + tab_h;
        let queue_tab_bar_top = queue_tab_title_top - (tab_h - queue_tab_title_h) / 2.0;
        let body_bottom = queue_tab_bar_top;
        let body_center = (body_top + body_bottom) / 2.0;

        // 收集正文区内高度约 33px 的控件矩形（发送按钮 / 模式下拉框 / 选择文件按钮）
        for clipped in &output.shapes {
            let rect = match &clipped.shape {
                egui::epaint::Shape::Rect(rs) => Some(rs.rect),
                egui::epaint::Shape::Path(p) if !p.points.is_empty() => {
                    Some(egui::Rect::from_points(&p.points))
                }
                _ => None,
            };
            if let Some(r) = rect
                && (r.height() - 33.0).abs() < 4.0
                && r.min.y >= body_top - 1.0
                && r.max.y <= body_bottom + 1.0
            {
                file_buttons.push(r.center().y);
            }
        }
        assert_eq!(
            file_buttons.len(),
            3,
            "文件面板正文中应恰好有 3 个 33px 高控件（发送/模式/选择文件）"
        );
        // 正文区高度应等于 FILE_BODY_H，且不低于 ScrollArea 最小内容高度
        let body_h = body_bottom - body_top;
        assert!(
            (body_h - FILE_BODY_H).abs() < 2.0,
            "正文区高度 {body_h:.1} 与 FILE_BODY_H {FILE_BODY_H} 不符"
        );
        // 每个控件的垂直中心都应落在正文区中心（真正的垂直居中）
        for center in &file_buttons {
            assert!(
                (center - body_center).abs() < 1.0,
                "控件中心 y={center:.1} 未居中：正文区 {body_top:.1}..{body_bottom:.1}，中心 {body_center:.1}"
            );
        }
    }

    #[test]
    fn send_row_periodic_does_not_overlap_history() {
        // 中英文下，Send 区右侧“定时发送”组都不能左溢盖住 History 下拉框。
        for lang in [Language::Chinese, Language::English] {
            let ctx = egui::Context::default();
            setup_fonts(&ctx);
            theme::apply(&ctx);
            let mut app = make_app();
            app.config.language = lang;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 70.0),
                )),
                ..Default::default()
            };
            let output = run_ui(&ctx, input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.set_width(1084.0);
                    app.send_buttons_row(ui);
                });
            });

            // History 下拉框空内容时显示“—”，Periodic 复选框文本随语言变化
            let periodic_text = if lang == Language::English {
                "Periodic"
            } else {
                "定时发送"
            };
            let (mut dash_right, mut periodic_left) = (f32::MIN, f32::MAX);
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                    let text = &ts.galley.job.text;
                    if text == "—" {
                        dash_right = dash_right.max(ts.pos.x + ts.galley.rect.width());
                    } else if text == periodic_text {
                        periodic_left = periodic_left.min(ts.pos.x);
                    }
                }
            }
            assert!(
                dash_right > f32::MIN && periodic_left < f32::MAX,
                "未找到 History 下拉框或 Periodic 复选框文本（{lang:?}）"
            );
            // 下拉框右缘 ≈ 破折号右缘 + 左内边距 8 + 右侧箭头区 26；
            // 复选框左缘 ≈ 文字左缘 − 复选框图标(18) − 图标间距(8)。
            let hist_right = dash_right + 8.0 + 26.0;
            let checkbox_left = periodic_left - 18.0 - 8.0;
            assert!(
                checkbox_left >= hist_right - 0.5,
                "{lang:?} 下 Periodic 复选框盖住 History 下拉框：\
                 checkbox_left={checkbox_left:.1} < hist_right={hist_right:.1}"
            );
        }
    }

    #[test]
    fn send_row_periodic_group_vertically_centered() {
        // 中英文下，Periodic 复选框、Interval 标签、数值框、“ms” 的文字中心必须对齐
        // （此前 egui 的 interact_size.y=30 让复选框/数值框溢出分配高度、中心下移）。
        for lang in [Language::Chinese, Language::English] {
            let ctx = egui::Context::default();
            setup_fonts(&ctx);
            theme::apply(&ctx);
            let mut app = make_app();
            app.config.language = lang;
            let periodic_text = if lang == Language::English {
                "Periodic"
            } else {
                "定时发送"
            };
            let interval_text = if lang == Language::English {
                "Interval"
            } else {
                "间隔"
            };
            let value_text = app.config.periodic_interval_ms.to_string();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 70.0),
                )),
                ..Default::default()
            };
            let output = run_ui(&ctx, input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.set_width(1084.0);
                    app.send_buttons_row(ui);
                });
            });

            let mut centers = Vec::new();
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                    let text = &ts.galley.job.text;
                    if text == "ms"
                        || text == periodic_text
                        || text == interval_text
                        || text == &value_text
                    {
                        centers.push(ts.pos.y + ts.galley.rect.height() / 2.0);
                    }
                }
            }
            assert_eq!(
                centers.len(),
                4,
                "应找到 Periodic/Interval/数值框/ms 四个文字（{lang:?}）"
            );
            let min = centers.iter().copied().fold(f32::MAX, f32::min);
            let max = centers.iter().copied().fold(f32::MIN, f32::max);
            assert!(
                max - min < 1.5,
                "{lang:?} 下定时发送组未垂直居中：中心 y 差异 {:.1}px（{centers:?}）",
                max - min
            );
        }
    }

    #[test]
    fn receive_rows_wrap_without_horizontal_overflow() {
        // 窄宽度 + 英文下，接收区设置行与标题行必须流式换行，不能横向溢出。
        for lang in [Language::Chinese, Language::English] {
            let ctx = egui::Context::default();
            setup_fonts(&ctx);
            theme::apply(&ctx);
            let mut app = make_app();
            app.config.language = lang;
            let w = 320.0; // 较窄：英文下设置行必然换行
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, 260.0),
                )),
                ..Default::default()
            };
            let output = run_ui(&ctx, input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.set_width(w - 16.0); // CentralPanel 默认内边距 8×2
                    app.receive_settings_row(ui);
                    ui.add_space(8.0);
                    app.receive_area(ui);
                });
            });
            let right = w - 8.0;
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(ts) = &clipped.shape {
                    let max_x = ts.pos.x + ts.galley.rect.width();
                    assert!(
                        max_x <= right + 2.0,
                        "{lang:?} 下接收区控件横向溢出：max_x={max_x:.1} > {right:.1}（文本：{}）",
                        ts.galley.job.text
                    );
                }
            }
        }
    }

    #[test]
    fn local_echo_controls_tx_display_in_non_terminal_mode() {
        // 本地回显（原“显示发送”）在非终端模式下控制发送数据显示：
        // 开启时记录 [TX] 段，关闭后不再显示。
        let mut app = make_app();
        app.config.terminal_mode = false;
        app.handle_event(Event::Tx(b"world".to_vec()));
        assert_eq!(app.display_segments.len(), 1);
        assert!(
            app.display_segments[0].marker.contains("[TX]"),
            "本地回显开启时应记录 [TX] 段"
        );
        app.config.terminal_auto_echo = false;
        app.display_segments.clear();
        app.handle_event(Event::Tx(b"again".to_vec()));
        assert!(
            app.display_segments.is_empty(),
            "关闭本地回显后不应显示发送数据"
        );
    }

    #[test]
    fn send_panel_symmetric_padding_and_internal_scroll() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        let mut app = make_app();
        app.config.language = Language::Chinese;
        app.send_input = "line\n".repeat(80); // 触发输入框内部滚动
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 740.0),
            )),
            ..Default::default()
        };
        let output = run_ui(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                let config_body_h = app.config_panel_body_h;
                let open_panels = current_open_panels(&dock);
                let mut viewer = DockViewer {
                    app: &mut app,
                    open_panels,
                    pending_restore: None,
                };
                let style = dock_style(ui);
                fix_file_panel_height(
                    &mut dock,
                    ui.available_height(),
                    style.tab_bar.height + FILE_BODY_H,
                );
                fix_config_panel_height(
                    &mut dock,
                    ui.available_height(),
                    style.tab_bar.height + config_body_h,
                );
                DockArea::new(&mut dock)
                    .style(style)
                    .show_add_buttons(false)
                    .show_add_popup(false)
                    .show_close_buttons(false)
                    .show_leaf_collapse_buttons(true)
                    .show_leaf_close_all_buttons(false)
                    .show_secondary_button_hint(false)
                    .tab_context_menus(false)
                    .show_inside(ui, &mut viewer);
                app.dock_state = Some(dock);
            });
        });
        let (send_node, _) = app
            .dock_state
            .as_ref()
            .unwrap()
            .main_surface()
            .find_tab(&UiPanel::Send)
            .unwrap();
        let viewport = app
            .dock_state
            .as_ref()
            .unwrap()
            .main_surface()
            .leaf(send_node)
            .unwrap()
            .viewport;
        // 发送按钮（主题蓝，位于面板内）：取其下缘
        let send_button_bottom = output
            .shapes
            .iter()
            .filter_map(|c| {
                if let egui::epaint::Shape::Rect(rs) = &c.shape {
                    (rs.fill == theme::BLUE
                        && rs.rect.top() >= viewport.top()
                        && rs.rect.bottom() <= viewport.bottom() + 1.0)
                        .then_some(rs.rect.bottom())
                } else {
                    None
                }
            })
            .max_by(|a, b| a.total_cmp(b))
            .expect("应找到发送按钮");
        // 输入框背景：面板内宽 > 500、高 24..90、位于按钮行上方的矩形
        // （不按颜色识别，避免并行测试主题竞态）
        let input_box = output
            .shapes
            .iter()
            .filter_map(|c| {
                if let egui::epaint::Shape::Rect(rs) = &c.shape {
                    (rs.rect.width() > 500.0
                        && (24.0..=90.0).contains(&rs.rect.height())
                        && rs.rect.top() >= viewport.top() - 1.0
                        && rs.rect.bottom() <= send_button_bottom + 1.0)
                        .then_some(rs.rect)
                } else {
                    None
                }
            })
            .max_by(|a, b| a.height().total_cmp(&b.height()))
            .expect("应找到发送输入框");
        // 顶部/底部可视留白一致
        let top_pad = input_box.top() - viewport.top();
        let bottom_pad = viewport.bottom() - send_button_bottom;
        assert!(
            (top_pad - bottom_pad).abs() < 2.0,
            "发送面板上下留白不一致: top={top_pad:.1} bottom={bottom_pad:.1}"
        );
        // 输入框不越过按钮行
        assert!(
            input_box.bottom() <= send_button_bottom - 2.0,
            "输入框与按钮行重叠"
        );
    }

    #[test]
    fn default_dock_file_slim_and_queue_min_height() {
        let ctx = egui::Context::default();
        setup_fonts(&ctx);
        theme::apply(&ctx);
        let mut app = make_app();
        app.config.language = Language::Chinese;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 740.0),
            )),
            ..Default::default()
        };
        let mut output = None;
        for _frame in 0..3 {
            output = Some(run_ui(&ctx, input.clone(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let mut dock = app.dock_state.take().expect("dock_state 应始终存在");
                    let config_body_h = app.config_panel_body_h;
                    let open_panels = current_open_panels(&dock);
                    let mut viewer = DockViewer {
                        app: &mut app,
                        open_panels,
                        pending_restore: None,
                    };
                    let style = dock_style(ui);
                    fix_file_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + FILE_BODY_H,
                    );
                    fix_config_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + config_body_h,
                    );
                    fix_queue_panel_height(
                        &mut dock,
                        ui.available_height(),
                        style.tab_bar.height + QUEUE_BODY_MIN,
                    );
                    DockArea::new(&mut dock)
                        .style(style)
                        .show_add_buttons(false)
                        .show_add_popup(false)
                        .show_close_buttons(false)
                        .show_leaf_collapse_buttons(true)
                        .show_leaf_close_all_buttons(false)
                        .show_secondary_button_hint(false)
                        .tab_context_menus(false)
                        .show_inside(ui, &mut viewer);
                    app.dock_state = Some(dock);
                });
            }));
        }
        let dock = app.dock_state.as_ref().unwrap();
        let body_of = |p: UiPanel| -> egui::Rect {
            let (n, _) = dock.main_surface().find_tab(&p).unwrap();
            dock.main_surface().leaf(n).unwrap().viewport
        };
        let file_body = body_of(UiPanel::File);
        let queue_body = body_of(UiPanel::Queue);
        let _ = output;
        assert!(
            (file_body.height() - FILE_BODY_H).abs() < 3.0,
            "文件面板正文高 {:.1} 应接近 FILE_BODY_H {FILE_BODY_H}",
            file_body.height()
        );
        assert!(
            queue_body.height() >= QUEUE_BODY_MIN - 2.0,
            "队列面板正文高 {:.1} 应不小于 QUEUE_BODY_MIN {QUEUE_BODY_MIN}",
            queue_body.height()
        );
    }

    #[test]
    fn queue_item_rows_auto_width_and_no_hidden_controls() {
        // 窗口收窄时，队列条目内容文本框必须“吃掉”剩余宽度整体收缩；
        // 右侧 发送/延时/移动/复制/删除 控件必须始终完整可见（不能被行尾裁掉）。
        let mut content_w = Vec::new();
        let mut seg_counts = Vec::new();
        for width in [1100.0f32, 700.0, 520.0, 460.0, 400.0] {
            let ctx = egui::Context::default();
            setup_fonts(&ctx);
            let mut app = make_app();
            app.config.language = Language::Chinese;
            let q = &mut app.config.queues[0];
            q.items.clear();
            q.items.push(crate::config::QueueItem {
                mode: crate::config::SendMode::Text,
                content: "hello queue item content long".to_owned(),
                delay_ms: 100,
                selected: true,
            });
            q.items.push(crate::config::QueueItem::default());
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 240.0),
                )),
                ..Default::default()
            };
            let mut out = None;
            for _ in 0..3 {
                out = Some(run_ui(&ctx, input.clone(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        app.queue_panel(ui);
                    });
                }));
            }
            let output = out.unwrap();
            let mut widths = Vec::new();
            let mut segments = 0usize;
            for clipped in &output.shapes {
                match &clipped.shape {
                    egui::epaint::Shape::Rect(rs) => {
                        // 内容文本框：位于行内 x≈128 起、高约 24px 的背景矩形
                        if rs.rect.left() > 100.0
                            && rs.rect.top() > 35.0
                            && (23.0..=26.0).contains(&rs.rect.height())
                        {
                            widths.push(rs.rect.width());
                        }
                    }
                    egui::epaint::Shape::LineSegment { .. } => segments += 1,
                    _ => {}
                }
            }
            assert_eq!(
                widths.len(),
                2,
                "width={width} 应渲染出 2 个队列条目的内容文本框"
            );
            let max_w = widths.into_iter().fold(f32::MIN, f32::max);
            content_w.push((width, max_w));
            seg_counts.push((width, segments));
            if width >= 460.0 {
                assert!(
                    segments > 0,
                    "width={width} 下队列行右侧图标控件应完整绘制"
                );
            }
        }
        // 内容文本框宽度随窗口收窄而同步收缩（1100 > 700 > 520 > 460）
        for pair in content_w.windows(2) {
            let (_, a) = pair[0];
            let (w2, b) = pair[1];
            assert!(
                a > b,
                "宽度 {:.0}px 时内容文本框 {a:.1}px 未比 {:.0}px 时的 {b:.1}px 更窄",
                pair[0].0, w2
            );
        }
        // 460px（仍放得下最小内容框）时右侧控件绘制数量与 700px 完全一致，未被裁剪
        let seg_700 = seg_counts[1].1;
        let seg_460 = seg_counts[3].1;
        assert_eq!(
            seg_460, seg_700,
            "460px 下右侧图标绘制数 {seg_460} 应与 700px 的 {seg_700} 一致（不得被裁剪）"
        );
    }

}
