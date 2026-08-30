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

/// “发送文件”面板正文的固定高度。
///
/// 控件行高 33px，加上 egui_dock 正文外框默认 8px 上下内边距后约 49px；
/// 但 egui_dock 的 tab 正文用 `ScrollArea::new([true, true])` 渲染，
/// 而 egui 0.35 的 ScrollArea 有 `min_scrolled_size = 64px` 的最小内容高度：
/// 若正文高度低于 64px，内容区会被强制撑到 64px 再垂直居中，导致控件被下推、
/// 底部超出面板被裁掉（表现为“控件贴着下边沿”）。
/// 因此这里取 66px（略高于 64px，并留出像素取整余量），使 ScrollArea 不再撑高、
/// 行内 `Align::Center` 才能真正垂直居中。
const FILE_BODY_H: f32 = 66.0;

/// 可停靠面板：接收区 / 发送区 / 文件发送 / 队列。
///
/// 每个面板在 [`DockState`] 中是一个 tab，支持拖动、拆分、关闭（收起）与恢复。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiPanel {
    Receive,
    Send,
    File,
    Queue,
}

impl UiPanel {
    /// 全部面板，顺序即默认 tab 顺序。
    pub const ALL: [UiPanel; 4] = [
        UiPanel::Receive,
        UiPanel::Send,
        UiPanel::File,
        UiPanel::Queue,
    ];

    /// 当前语言下的面板标题。
    pub fn title(self, s: &Strings) -> &'static str {
        match self {
            UiPanel::Receive => s.receive_area,
            UiPanel::Send => s.send,
            UiPanel::File => s.send_file,
            UiPanel::Queue => s.queue,
        }
    }
}

/// 默认停靠布局：沿用原有纵向堆叠的观感（接收区在上，发送/文件/队列在下），
/// 每个区域独立成一块，可拖动、拆分、关闭与恢复。
fn default_dock_state() -> DockState<UiPanel> {
    let mut dock = DockState::new(vec![UiPanel::Receive]);
    let root = NodeIndex::root();
    let [_, send] = dock
        .main_surface_mut()
        .split_below(root, 0.49, vec![UiPanel::Send]);
    let [_, file] = dock
        .main_surface_mut()
        .split_below(send, 0.41, vec![UiPanel::File]);
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
                // 输入框填充剩余空间，按钮行固定在底部
                let input_h = (ui.available_height() - 44.0 - 6.0).max(40.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), input_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.send_input_box(ui),
                );
                ui.add_space(6.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 44.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.send_buttons_row(ui),
                );
            }
            UiPanel::File => self.file_panel(ui),
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
                if self.config.show_sent_data {
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
        // 布局1：串口配置（顶部通栏，内容横向分布、高度固定、宽度自适应）
        egui::Panel::top("serial_config")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
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
                    .fill(theme::status_bg())
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                self.status_bar(ui);
            });

        // 主布局：四个区域以停靠 tab 呈现，可拖动、拆分、关闭（收起）与恢复。
        // 先取出 dock_state，使 viewer 可以独占借用 self，渲染结束后再放回。
        let mut dock = self.dock_state.take().expect("dock_state 应始终存在");
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
    open_panels: [bool; 4],
    /// “+” 菜单中点击待恢复的面板及其所属分组，渲染结束后应用到 dock_state。
    pending_restore: Option<(UiPanel, NodeIndex)>,
}

impl TabViewer for DockViewer<'_> {
    type Tab = UiPanel;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title(self.app.t()).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.app.panel_ui(ui, *tab);
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
fn current_open_panels(dock: &DockState<UiPanel>) -> [bool; 4] {
    let mut open = [false; 4];
    for tab in dock.main_surface().tabs() {
        match tab {
            UiPanel::Receive => open[0] = true,
            UiPanel::Send => open[1] = true,
            UiPanel::File => open[2] = true,
            UiPanel::Queue => open[3] = true,
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
            dock_state: Some(default_dock_state()),
        }
    }

    #[test]
    fn default_dock_state_contains_all_panels() {
        let dock = default_dock_state();
        let tabs: Vec<UiPanel> = dock.main_surface().tabs().copied().collect();
        assert_eq!(tabs.len(), 4);
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
        assert!(!open[3]);
        assert!(open[0] && open[1] && open[2]);
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
        // 正文高度必须不低于 egui 0.35 ScrollArea 的最小内容高度（64px），
        // 否则内容区被强制撑高、控件会被下推并贴到面板下边沿。
        // 编译期不变量：正文高度必须不低于 egui 0.35 ScrollArea 的最小内容高度（64px），
        // 否则内容区被强制撑高、控件会被下推并贴到面板下边沿。
        const _: () = assert!(
            FILE_BODY_H >= 64.0,
            "FILE_BODY_H 低于 ScrollArea 最小内容高度 64px，控件无法垂直居中"
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
        let output = ctx.run_ui(input, |ui| {
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
                } else if text == s.queue && ts.pos.y < queue_tab_title_top {
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
            let output = ctx.run_ui(input, |ui| {
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
            let output = ctx.run_ui(input, |ui| {
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
            let output = ctx.run_ui(input, |ui| {
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
}
