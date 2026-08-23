//! 顶部各区：串口配置（横向分布、填满窗口）、显示/日志设置行、文件发送行。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版，800×700 单栏布局）。

use crate::app::SerialApp;
use crate::config::{
    DataBits, DisplayMode, FileSendMode, FlowControl, Parity, StopBits, TextEncoding,
};
use crate::i18n::Language;
use crate::serial::Command;
use crate::ui::widgets;
use crate::ui::theme;
use eframe::egui;
use std::path::PathBuf;

impl SerialApp {
    /// 串口配置区：6 个字段子布局 + “打开串口”/“语言”按钮，
    /// 以相同间距横向铺满窗口；端口下拉框自适应吸收剩余宽度。
    pub fn config_panel(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        let enabled = !self.session_connected;
        let avail = ui.available_width();
        const GAP: f32 = 8.0; // 8 个单元之间的外部间距
        const DATA_W: f32 = 52.0;
        const STOP_W: f32 = 66.0;
        const PARITY_W: f32 = 74.0;
        const FLOW_W: f32 = 84.0;
        const BAUD_FRAME_W: f32 = 88.0; // 波特率输入框（含边框与内部边距）总宽
        const BAUD_MARGIN_X: f32 = 8.0; // 输入框内部左右边距（4+4）
        const ARROW_W: f32 = 20.0;
        const BTN_W: f32 = 88.0;
        const LANG_W: f32 = 80.0;
        const PORT_MIN_COMBO_W: f32 = 100.0;
        const PORT_GROWTH: f32 = 0.20; // 窗口变宽时端口最多再增加初始宽度的 20%
        const PORT_HEADROOM: f32 = 30.0; // 端口初始宽度预留余量，保证语言按钮完整显示
        /// 标准窗口（1100px）下的面板可用宽度，用于计算端口下拉框的基准宽度
        const AVAIL_REF: f32 = 1084.0;

        // 测量标签文字宽度：端口按英文宽度固定，其余取中英文最大宽度
        let measure = |ui: &mut egui::Ui, text: &str| -> f32 {
            let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
            ui.fonts_mut(|f| {
                f.layout_no_wrap(text.to_owned(), font_id, egui::Color32::WHITE)
            })
            .size()
            .x
        };
        let en = Language::English.strings();
        let port_label_w = measure(ui, en.port);
        let baud_label_w = measure(ui, s.baud_rate).max(measure(ui, en.baud_rate));
        let data_label_w = measure(ui, s.data_bits).max(measure(ui, en.data_bits));
        let stop_label_w = measure(ui, s.stop_bits).max(measure(ui, en.stop_bits));
        let parity_label_w = measure(ui, s.parity).max(measure(ui, en.parity));
        let flow_label_w = measure(ui, s.flow_control).max(measure(ui, en.flow_control));

        let baud_field_w = baud_label_w + FIELD_INNER + BAUD_FRAME_W;
        let data_field_w = data_label_w + FIELD_INNER + DATA_W;
        let stop_field_w = stop_label_w + FIELD_INNER + STOP_W;
        let parity_field_w = parity_label_w + FIELD_INNER + PARITY_W;
        let flow_field_w = flow_label_w + FIELD_INNER + FLOW_W;
        let fixed_units = baud_field_w
            + data_field_w
            + stop_field_w
            + parity_field_w
            + flow_field_w
            + BTN_W
            + LANG_W;
        // 端口下拉框：初始宽度按标准窗口计算，控件间隔固定；
        // 窗口变宽时端口最多再增加初始宽度的 20%，其余空间留在行尾
        let port_base_w = (AVAIL_REF
            - fixed_units
            - 7.0 * GAP
            - (port_label_w + FIELD_INNER)
            - PORT_HEADROOM)
            .max(PORT_MIN_COMBO_W);
        let port_max_w = port_base_w * (1.0 + PORT_GROWTH);
        let extra = (avail - AVAIL_REF).max(0.0);
        let mut port_combo_w = (port_base_w + extra).min(port_max_w);
        // 兜底：窗口过窄时收缩端口宽度
        let overflow = fixed_units + port_label_w + FIELD_INNER + port_combo_w + 7.0 * GAP - avail;
        if overflow > 0.0 {
            port_combo_w = (port_combo_w - overflow).max(PORT_MIN_COMBO_W);
        }

        ui.spacing_mut().item_spacing.x = GAP;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            // 子布局1：端口（label 固定，下拉框自适应）
            ui.allocate_ui_with_layout(
                egui::vec2(port_label_w + FIELD_INNER + port_combo_w, 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = FIELD_INNER;
                    ui.add_sized(
                        [port_label_w, 28.0],
                        egui::Label::new(s.port).halign(egui::Align::Min),
                    )
                    .on_hover_text(s.port_tip);
                    let port = if self.config.port.port_name.is_empty() {
                        s.port_placeholder.to_string()
                    } else {
                        self.port_list
                            .iter()
                            .find(|(_, n)| n == &self.config.port.port_name)
                            .map(|(d, _)| d.clone())
                            .unwrap_or_else(|| self.config.port.port_name.clone())
                    };
                    widgets::combo(
                        ui,
                        ui.available_width(),
                        28.0,
                        enabled,
                        port,
                        s.port_tip,
                        |ui| {
                            for (display, name) in &self.port_list {
                                ui.selectable_value(
                                    &mut self.config.port.port_name,
                                    name.clone(),
                                    display,
                                );
                            }
                        },
                    );
                },
            );

            // 子布局2：波特率（label 固定；文本框自适应 + 固定宽下拉按钮）
            ui.allocate_ui_with_layout(
                egui::vec2(baud_field_w, 28.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = FIELD_INNER;
                    ui.add_sized(
                        [baud_label_w, 28.0],
                        egui::Label::new(s.baud_rate).halign(egui::Align::Min),
                    )
                    .on_hover_text(s.baud_rate_tip);
                    let text_w =
                        (BAUD_FRAME_W - BAUD_MARGIN_X - ARROW_W - FIELD_INNER).max(40.0);
                    let mut arrow_resp = None;
                    egui::Frame::new()
                        .fill(egui::Color32::WHITE)
                        .stroke(theme::BORDER)
                        .corner_radius(4)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.x = FIELD_INNER;
                            let mut baud = self.baud_input.clone();
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut baud)
                                    .font(egui::TextStyle::Monospace)
                                    .frame(egui::Frame::NONE)
                                    .background_color(egui::Color32::TRANSPARENT)
                                    .desired_width(text_w),
                            );
                            if edit.changed() {
                                self.baud_input = baud;
                                if let Ok(v) = self.baud_input.trim().parse::<u32>() {
                                    self.config.port.baud_rate = v;
                                }
                            }
                            widgets::text_edit_context_menu(
                                ui,
                                &edit,
                                true,
                                &self.baud_input,
                                self.t(),
                            );
                            edit.on_hover_text(s.baud_rate);
                            let arrow = ui
                                .add_sized(
                                    [ARROW_W, 20.0],
                                    egui::Button::new("")
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_text(s.common_bauds);
                            if ui.is_rect_visible(arrow.rect) {
                                let tri = egui::Rect::from_center_size(
                                    arrow.rect.center(),
                                    egui::vec2(6.0, 4.0),
                                );
                                ui.painter().add(egui::Shape::convex_polygon(
                                    vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
                                    theme::TEXT_SOFT,
                                    egui::Stroke::NONE,
                                ));
                            }
                            arrow_resp = Some(arrow);
                        });
                    if let Some(arrow) = arrow_resp {
                        egui::Popup::menu(&arrow).show(|ui| {
                        for rate in [
                            9600u32, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
                            1_000_000, 2_000_000,
                        ] {
                            if ui
                                .selectable_label(
                                    self.config.port.baud_rate == rate,
                                    rate.to_string(),
                                )
                                .clicked()
                            {
                                self.baud_input = rate.to_string();
                                self.config.port.baud_rate = rate;
                                ui.close();
                            }
                        }
                        });
                    }
                },
            );

            // 子布局3~6：数据位 / 停止位 / 校验位 / 流控（label 与下拉框宽度均固定）
            field_ui(
                ui,
                s.data_bits,
                data_label_w,
                DATA_W,
                enabled,
                self.config.port.data_bits.label().to_string(),
                s.data_bits_tip,
                |ui| {
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Five, "5");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Six, "6");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Seven, "7");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Eight, "8");
                },
            );
            field_ui(
                ui,
                s.stop_bits,
                stop_label_w,
                STOP_W,
                enabled,
                self.config.port.stop_bits.label().to_string(),
                s.stop_bits_tip,
                |ui| {
                    ui.selectable_value(&mut self.config.port.stop_bits, StopBits::One, "1");
                    ui.selectable_value(
                        &mut self.config.port.stop_bits,
                        StopBits::OnePointFive,
                        "1.5",
                    );
                    ui.selectable_value(&mut self.config.port.stop_bits, StopBits::Two, "2");
                },
            );
            field_ui(
                ui,
                s.parity,
                parity_label_w,
                PARITY_W,
                enabled,
                self.config.port.parity.label().to_string(),
                s.parity_tip,
                |ui| {
                    ui.selectable_value(&mut self.config.port.parity, Parity::None, "None");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Even, "Even");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Odd, "Odd");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Mark, "Mark");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Space, "Space");
                },
            );
            let flow = match self.config.port.flow_control {
                FlowControl::None => s.flow_none,
                FlowControl::Software => s.flow_software,
                FlowControl::Hardware => s.flow_hardware,
            };
            field_ui(
                ui,
                s.flow_control,
                flow_label_w,
                FLOW_W,
                enabled,
                flow.to_string(),
                s.flow_tip,
                |ui| {
                    ui.selectable_value(
                        &mut self.config.port.flow_control,
                        FlowControl::None,
                        s.flow_none,
                    );
                    ui.selectable_value(
                        &mut self.config.port.flow_control,
                        FlowControl::Software,
                        s.flow_software,
                    );
                    ui.selectable_value(
                        &mut self.config.port.flow_control,
                        FlowControl::Hardware,
                        s.flow_hardware,
                    );
                },
            );

            // 打开/关闭串口
            let label = if self.session_connected {
                s.close_port
            } else {
                s.open_port
            };
            if ui
                .add_sized([BTN_W, 30.0], theme::primary_widget(label))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.open_close_tip)
                .clicked()
            {
                if self.session_connected {
                    self.session.send(Command::Close);
                } else {
                    self.open_port();
                }
            }

            // 最右：语言按钮（固定宽度，高度与“打开串口”按钮一致）
            let lang_btn = ui
                .add_sized([LANG_W, 30.0], theme::secondary_widget(s.language))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.language_tip);
            egui::Popup::menu(&lang_btn).show(|ui| {
                for lang in [Language::Chinese, Language::English] {
                    if ui
                        .selectable_label(self.config.language == lang, lang.label())
                        .clicked()
                    {
                        self.set_language(lang);
                        ui.close();
                    }
                }
            });
        });
    }

    /// 布局2-子1：接收功能设置行（横向分布、高度固定、宽度自适应、无边框）。
    pub fn receive_settings_row(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(s.display).color(theme::TEXT_SOFT));
            widgets::combo(
                ui,
                80.0,
                26.0,
                true,
                self.config.display_mode.label(self.config.language),
                s.display_tip,
                |ui| {
                    if ui
                        .selectable_value(
                            &mut self.config.display_mode,
                            DisplayMode::Text,
                            s.text_mode,
                        )
                        .changed()
                        || ui
                            .selectable_value(
                                &mut self.config.display_mode,
                                DisplayMode::Hex,
                                "HEX",
                            )
                            .changed()
                    {
                        self.display_cache_invalid = true;
                        self.display_dirty = true;
                    }
                },
            );
            ui.label(egui::RichText::new(s.encoding).color(theme::TEXT_SOFT));
            widgets::combo(
                ui,
                86.0,
                26.0,
                true,
                self.config.text_encoding.label(),
                s.encoding_tip,
                |ui| {
                    if ui
                        .selectable_value(
                            &mut self.config.text_encoding,
                            TextEncoding::Utf8,
                            "UTF-8",
                        )
                        .changed()
                        || ui
                            .selectable_value(
                                &mut self.config.text_encoding,
                                TextEncoding::Gbk,
                                "GBK",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.config.text_encoding,
                                TextEncoding::Ascii,
                                "ASCII",
                            )
                            .changed()
                    {
                        self.display_cache_invalid = true;
                        self.display_dirty = true;
                    }
                },
            );
            ui.checkbox(&mut self.config.autoscroll, s.auto_scroll)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.auto_scroll_tip);
            ui.checkbox(&mut self.paused, s.pause_receive)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.pause_receive_tip);
            ui.checkbox(&mut self.config.auto_refresh_ports, s.auto_refresh)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.auto_refresh_tip);
            ui.checkbox(&mut self.config.show_sent_data, s.show_sent)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.show_sent_tip);

            // 记录日志：勾选后自动弹出文件保存对话框选择日志路径
            let log_resp = ui
                .checkbox(&mut self.log_on, s.record_log)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.record_log_tip);
            if log_resp.changed() {
                if self.log_on {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(s.log_filter, &["log", "txt"])
                        .set_file_name("serial.log")
                        .save_file()
                    {
                        self.set_log_path(Some(path));
                    } else {
                        self.log_on = false;
                    }
                } else {
                    self.close_log();
                }
            }

            // 打开日志路径：仅当“记录日志”勾选后启用
            let log_path = self.log_path.clone();
            if ui
                .add_enabled(
                    self.log_on,
                    egui::Button::new(s.open_log_path)
                        .min_size(egui::vec2(88.0, 32.0)),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if self.log_on {
                    s.open_log_path_tip
                } else {
                    s.enable_log_first
                })
                .clicked()
            {
                open_log_folder_path(log_path.as_deref());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized([72.0, 32.0], theme::secondary_widget(s.clear_stats))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.clear_stats_tip)
                    .clicked()
                {
                    self.rx_total = 0;
                    self.tx_total = 0;
                }
                if ui
                    .add_sized([72.0, 32.0], theme::secondary_widget(s.clear_display))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.clear_display_tip)
                    .clicked()
                {
                    self.clear_display();
                }
            });
        });
    }

    /// 文件发送行。
    pub fn file_panel(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if ui
                .add_sized([102.0, 33.0], theme::primary_widget(s.send_file))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.send_file_tip)
                .clicked()
            {
                self.start_file_send();
            }
            // 统一样式下拉框：宽度与发送文件按钮一致（102px），高度相同（33px）
            widgets::combo(
                ui,
                102.0,
                33.0,
                true,
                self.config.file_mode.label(self.config.language),
                s.file_mode_tip,
                |ui| {
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::WholeFile,
                        s.whole_file,
                    );
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::LineByLine,
                        s.line_by_line,
                    );
                },
            );
            // 逐行发送时：行间隔控件紧跟在模式下拉框后面
            if self.config.file_mode == FileSendMode::LineByLine {
                ui.label(egui::RichText::new(s.line_interval).color(theme::TEXT_SOFT));
                let resp = ui.add_sized(
                    [80.0, 24.0],
                    egui::DragValue::new(&mut self.config.file_line_interval_ms)
                        .range(0..=60_000)
                        .suffix(" ms"),
                );
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.line_interval_tip);
            }
            // 选择文件按钮：固定较宽宽度，选中文件后直接显示文件名
            let name = self
                .pending_file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned());
            let btn_text = match &name {
                Some(n) => shorten_file_name(n, 22),
                None => s.choose_file.to_string(),
            };
            if ui
                .add_sized([180.0, 33.0], theme::secondary_widget(&btn_text))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.choose_file_tip)
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                self.pending_file = Some(path);
            }
            // 进度条：仅在发送时显示，占满“选择文件”按钮后的剩余宽度
            if self.file_send_active {
                let spacing = ui.spacing().item_spacing.x;
                let cancel_w = 72.0 + spacing;
                let (sent, total) = self.file_progress.unwrap_or((0, 0));
                let frac = if total > 0 {
                    sent as f32 / total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width((ui.available_width() - cancel_w).max(60.0))
                        .text(s.fill(
                            s.progress_bytes,
                            &[("sent", sent.to_string()), ("total", total.to_string())],
                        )),
                );
                if ui
                    .add_sized([72.0, 33.0], theme::primary_widget(s.cancel))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.cancel_send_tip)
                    .clicked()
                {
                    self.session.send(Command::StopFile);
                }
            }
        });
    }

    pub fn refresh_ports(&mut self) {
        match serialport::available_ports() {
            Ok(ports) => {
                let mut list: Vec<(String, String)> = ports
                    .into_iter()
                    .map(|info| (port_display(&info), info.port_name))
                    .collect();
                list.sort_by(|a, b| a.1.cmp(&b.1));
                let prev = self.config.port.port_name.clone();
                self.port_list = list;
                if self.port_list.is_empty() {
                    self.config.port.port_name.clear();
                } else if !self.port_list.iter().any(|(_, n)| n == &prev) {
                    self.config.port.port_name = self.port_list[0].1.clone();
                }
            }
            Err(e) => {
                self.set_status(
                    self.t()
                        .fill(self.t().enum_ports_failed, &[("e", e.to_string())]),
                    true,
                );
            }
        }
    }

    fn open_port(&mut self) {
        let port_name = self.config.port.port_name.trim().to_string();
        if port_name.is_empty() {
            self.set_status(self.t().select_port_first.to_string(), true);
            return;
        }
        let baud: u32 = match self.baud_input.trim().parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.set_status(self.t().invalid_baud.to_string(), true);
                return;
            }
        };
        self.config.port.baud_rate = baud;
        self.config.port.port_name = port_name;
        let settings = self.config.port.clone();
        self.session.send(Command::Open(settings));
        self.set_status(self.t().opening_port.to_string(), false);
    }

    pub fn set_log_path(&mut self, path: Option<PathBuf>) {
        self.close_log();
        self.log_path = path.clone();
        if let Some(p) = &path {
            match std::fs::File::create(p) {
                Ok(file) => {
                    self.log_file = Some(std::io::BufWriter::new(file));
                    self.log_on = true;
                }
                Err(e) => {
                    self.log_on = false;
                    self.set_status(
                        self.t()
                            .fill(self.t().create_log_failed, &[("e", e.to_string())]),
                        true,
                    );
                }
            }
        } else {
            self.log_on = false;
        }
    }

    pub fn close_log(&mut self) {
        self.log_file = None;
    }
}

/// 字段内部 label 与控件之间的间距。
const FIELD_INNER: f32 = 6.0;

/// 固定宽度字段子布局：固定宽 label + 固定宽下拉框（数据位/停止位/校验位/流控）。
#[allow(clippy::too_many_arguments)]
fn field_ui(
    ui: &mut egui::Ui,
    label: &str,
    label_w: f32,
    combo_w: f32,
    enabled: bool,
    selected: String,
    tip: &str,
    options: impl FnOnce(&mut egui::Ui),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(label_w + FIELD_INNER + combo_w, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = FIELD_INNER;
            ui.add_sized(
                [label_w, 28.0],
                egui::Label::new(label).halign(egui::Align::Min),
            )
            .on_hover_text(tip);
            widgets::combo(ui, combo_w, 28.0, enabled, selected, tip, options);
        },
    );
}

/// 文件名过长时保留末尾（含扩展名），避免撑破固定宽度的“选择文件”按钮。
fn shorten_file_name(name: &str, max_chars: usize) -> String {
    let count = name.chars().count();
    if count <= max_chars {
        name.to_string()
    } else {
        let keep = max_chars.saturating_sub(1);
        let tail: String = name.chars().skip(count - keep).collect();
        format!("…{tail}")
    }
}

/// 端口显示名：优先 USB 产品名/制造商；无名称时直接显示端口名（不在尾部追加 COMx）。
fn port_display(info: &serialport::SerialPortInfo) -> String {
    let friendly = match &info.port_type {
        serialport::SerialPortType::UsbPort(usb) => usb
            .product
            .as_deref()
            .or(usb.manufacturer.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    };
    if friendly.is_empty() {
        info.port_name.clone()
    } else {
        friendly
    }
}

/// 在系统文件管理器中打开日志文件所在目录（Windows 定位到文件，其他平台打开目录）。
fn open_log_folder_path(path: Option<&std::path::Path>) {
    let Some(path) = path else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let dir = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

#[cfg(test)]
mod tests {
    use crate::app::SerialApp;
    use crate::config::Config;
    use crate::i18n::Language;
    use crate::serial::SerialSession;
    use std::sync::Arc;
    use std::time::Instant;

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
        }
    }

    /// 与应用启动时一致的 Context：默认字体 + CJK 字体优先。
    /// 渲染测试必须使用真实字体，否则中文文本在无 CJK 字体的环境中
    /// （如 CI 的 macOS/Windows runner）渲染结果会不一致。
    fn cjk_ctx() -> eframe::egui::Context {
        let ctx = eframe::egui::Context::default();
        let mut fonts = eframe::egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".to_owned(),
            Arc::new(eframe::egui::FontData::from_static(include_bytes!(
                "../../assets/fonts/NotoSansCJKsc-Regular.otf"
            ))),
        );
        if let Some(list) = fonts.families.get_mut(&eframe::egui::FontFamily::Proportional) {
            list.insert(0, "cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        ctx
    }

    /// 收集渲染输出中所有文本及其 x 位置。
    fn text_positions(output: &eframe::egui::FullOutput) -> Vec<(String, f32, f32)> {
        let mut out = Vec::new();
        for clipped in &output.shapes {
            if let eframe::egui::epaint::Shape::Text(ts) = &clipped.shape {
                out.push((ts.galley.job.text.clone(), ts.pos.x, ts.galley.rect.width()));
            }
        }
        out
    }

    #[test]
    fn config_panel_layout_keeps_all_controls() {
        let ctx = cjk_ctx();

        for lang in [Language::Chinese, Language::English] {
            let mut app = make_app();
            app.config.language = lang;
            let output = ctx.run_ui(Default::default(), |ui| {
                // 模拟 1100px 窗口内的顶部面板（1100 - 左右边距 16）
                ui.allocate_ui_with_layout(
                    eframe::egui::vec2(1084.0, 48.0),
                    eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                    |ui| app.config_panel(ui),
                );
            });
            let s = lang.strings();
            let positions = text_positions(&output);
            let find = |label: &str| {
                positions
                    .iter()
                    .find(|(t, _, _)| t == label)
                    .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
            };
            // 从左到右的顺序：端口 < 波特率 < 数据位 < 停止位 < 校验位 < 流控 < 打开串口 < 语言
            let order = [
                find(s.port).1,
                find(s.baud_rate).1,
                find(s.data_bits).1,
                find(s.stop_bits).1,
                find(s.parity).1,
                find(s.flow_control).1,
                find(s.open_port).1,
                find(s.language).1,
            ];
            for w in order.windows(2) {
                assert!(
                    w[0] < w[1],
                    "[{lang:?}] 控件顺序/间距异常: x={} 不在 x={} 左侧",
                    w[0],
                    w[1]
                );
            }
            // 语言按钮必须完整落在行内（文本右缘不超过面板宽度）
            let (_, x, w) = *find(s.language);
            assert!(
                x + w <= 1084.0,
                "[{lang:?}] 语言按钮被挤出: x={x} w={w} (行宽 1084)"
            );
            // 整行从左到右填满：端口标签在行首，语言按钮靠近行尾
            assert!(
                find(s.port).1 < 1.0,
                "[{lang:?}] 端口标签未从行首开始: x={}",
                find(s.port).1
            );
            assert!(
                x + w >= 1084.0 - 75.0,
                "[{lang:?}] 语言按钮未靠右/整行未填满: 右缘={}",
                x + w
            );
            // 端口下拉框自适应吸收剩余空间：波特率标签应被推到较靠右的位置
            assert!(
                find(s.baud_rate).1 > 200.0,
                "[{lang:?}] 端口下拉框未吸收剩余宽度: 波特率标签 x={}",
                find(s.baud_rate).1
            );
        }
    }

    #[test]
    fn config_panel_language_button_visible_in_real_panel() {
        let ctx = cjk_ctx();

        for lang in [Language::Chinese, Language::English] {
            let mut app = make_app();
            app.config.language = lang;
            let input = eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::Pos2::ZERO,
                    eframe::egui::vec2(1100.0, 740.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                eframe::egui::Panel::top("serial_config")
                    .exact_size(48.0)
                    .frame(
                        eframe::egui::Frame::new()
                            .inner_margin(eframe::egui::Margin::same(8)),
                    )
                    .show(ui, |ui| {
                        app.config_panel(ui);
                    });
            });
            let s = lang.strings();
            let positions = text_positions(&output);
            let (_, x, w) = *positions
                .iter()
                .find(|(t, _, _)| t == s.language)
                .unwrap_or_else(|| panic!("[{lang:?}] 缺少语言按钮文本"));
            assert!(
                x + w <= 1100.0 - 4.0,
                "[{lang:?}] 语言按钮被挤出: 右缘={} (窗口 1100)",
                x + w
            );
        }
    }

    #[test]
    fn file_panel_layout_keeps_all_controls() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let s = Language::Chinese.strings();
        let positions = text_positions(&output);
        let x_of = |label: &str| {
            positions
                .iter()
                .find(|(t, _, _)| t == label)
                .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
                .1
        };
        // 顺序：发送文件 < 整块发送 < 选择文件
        let order = [
            x_of(s.send_file),
            x_of(s.whole_file),
            x_of(s.choose_file),
        ];
        for w in order.windows(2) {
            assert!(w[0] < w[1], "文件行控件顺序异常: x={} 不在 x={} 左侧", w[0], w[1]);
        }
        // 未发送时：不显示进度条文本与取消按钮
        assert!(!positions.iter().any(|(t, _, _)| t == s.cancel));
        assert!(!positions.iter().any(|(t, _, _)| t.contains('/')));
    }

    #[test]
    fn file_panel_button_shows_selected_file_name() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        // 使用平台无关的相对路径：Windows 上为 data\test.bin，
        // macOS/Linux 上为 data/test.bin，两者 file_name() 都返回 test.bin。
        app.pending_file = Some(std::path::Path::new("data").join("test.bin"));
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let positions = text_positions(&output);
        assert!(
            positions.iter().any(|(t, _, _)| t == "test.bin"),
            "选择文件按钮应显示文件名"
        );
    }

    #[test]
    fn file_panel_shows_progress_bar_while_sending() {
        let ctx = cjk_ctx();
        let mut app = make_app();
        app.file_send_active = true;
        app.file_progress = Some((50, 100));
        let output = ctx.run_ui(Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                eframe::egui::vec2(1084.0, 40.0),
                eframe::egui::Layout::left_to_right(eframe::egui::Align::Center),
                |ui| app.file_panel(ui),
            );
        });
        let s = Language::Chinese.strings();
        let positions = text_positions(&output);
        // 进度条文本与取消按钮在发送时出现
        assert!(
            positions.iter().any(|(t, _, _)| t == "50/100 字节"),
            "发送时应显示进度条文本"
        );
        assert!(positions.iter().any(|(t, _, _)| t == s.cancel));
    }

}
