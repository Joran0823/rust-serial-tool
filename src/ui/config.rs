//! 顶部各区：串口配置（横向分布、填满窗口）、显示/日志设置行、文件发送行。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版，800×700 单栏布局）。

use crate::app::SerialApp;
use crate::config::{
    DataBits, DisplayMode, FileSendMode, FlowControl, Parity, StopBits, TextEncoding,
};
use crate::serial::Command;
use crate::ui::widgets;
use crate::ui::theme;
use eframe::egui;
use std::path::PathBuf;

impl SerialApp {
    /// 串口配置区：单行六字段（端口/波特率/数据位/停止位/校验位/流控）+ 右侧按钮，
    /// 控件宽度按窗口宽度相对分配，随窗口缩放自适应并填满整行。
    pub fn config_panel(&mut self, ui: &mut egui::Ui) {
        let enabled = !self.session_connected;
        let avail = ui.available_width();
        let port_w = (avail * 0.17).clamp(110.0, 230.0);
        let baud_w = 108.0; // 固定宽度：足以完整显示 8 个字符
        let data_w = (avail * 0.05).clamp(36.0, 80.0);
        let stop_w = (avail * 0.05).clamp(62.0, 90.0);
        let parity_w = (avail * 0.07).clamp(44.0, 100.0);
        let flow_w = (avail * 0.08).clamp(52.0, 120.0);
        let btn_w = (avail * 0.10).clamp(76.0, 120.0);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let port = if self.config.port.port_name.is_empty() {
                "请选择".to_string()
            } else {
                self.port_list
                    .iter()
                    .find(|(_, n)| n == &self.config.port.port_name)
                    .map(|(d, _)| d.clone())
                    .unwrap_or_else(|| self.config.port.port_name.clone())
            };
            combo_field(
                ui,
                "端口",
                port_w,
                enabled,
                port,
                "选择要连接的串口",
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
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label("波特率").on_hover_text("串口通信速率（可输入，或从下拉选择常用值）");
                ui.add_enabled_ui(enabled, |ui| {
                    // 一体式可输入下拉框：带边框容器内包含无边框文本框 + 箭头按钮
                    let arrow = egui::Frame::new()
                        .fill(egui::Color32::WHITE)
                        .stroke(theme::BORDER)
                        .corner_radius(4)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            // 用固定 baud_w 计算文本框宽度，避免 Frame 内部 available_width
                            // 返回整行剩余宽度导致下拉框被撑满整行
                            let text_w =
                                (baud_w - 8.0 - 20.0 - ui.spacing().item_spacing.x).max(40.0);
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
                                widgets::text_edit_context_menu(ui, &edit, true, &self.baud_input);
                                edit.on_hover_text("串口通信速率");
                                ui.add_sized(
                                    [20.0, 20.0],
                                    egui::Button::new("")
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .on_hover_text("常用波特率")
                            })
                            .inner
                        })
                        .inner;
                    if ui.is_rect_visible(arrow.rect) {
                        let tri = egui::Rect::from_center_size(
                            egui::pos2(arrow.rect.center().x, arrow.rect.center().y),
                            egui::vec2(6.0, 4.0),
                        );
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![tri.left_top(), tri.right_top(), tri.center_bottom()],
                            theme::TEXT_SOFT,
                            egui::Stroke::NONE,
                        ));
                    }
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
                });
            });
            combo_field(
                ui,
                "数据位",
                data_w,
                enabled,
                self.config.port.data_bits.label().to_string(),
                "每帧数据包含的数据位数",
                |ui| {
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Five, "5");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Six, "6");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Seven, "7");
                    ui.selectable_value(&mut self.config.port.data_bits, DataBits::Eight, "8");
                },
            );
            combo_field(
                ui,
                "停止位",
                stop_w,
                enabled,
                self.config.port.stop_bits.label().to_string(),
                "每个数据帧后的停止位数量",
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
            combo_field(
                ui,
                "校验位",
                parity_w,
                enabled,
                self.config.port.parity.label().to_string(),
                "奇偶校验方式",
                |ui| {
                    ui.selectable_value(&mut self.config.port.parity, Parity::None, "None");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Even, "Even");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Odd, "Odd");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Mark, "Mark");
                    ui.selectable_value(&mut self.config.port.parity, Parity::Space, "Space");
                },
            );
            let flow = match self.config.port.flow_control {
                FlowControl::None => "无",
                FlowControl::Software => "软流控",
                FlowControl::Hardware => "硬流控",
            };
            combo_field(
                ui,
                "流控",
                flow_w,
                enabled,
                flow.to_string(),
                "串口流控方式（软流控/硬件流控）",
                |ui| {
                    ui.selectable_value(&mut self.config.port.flow_control, FlowControl::None, "无");
                    ui.selectable_value(
                        &mut self.config.port.flow_control,
                        FlowControl::Software,
                        "软流控",
                    );
                    ui.selectable_value(
                        &mut self.config.port.flow_control,
                        FlowControl::Hardware,
                        "硬流控",
                    );
                },
            );

            ui.add_space(8.0);
            let label = if self.session_connected {
                "关闭串口"
            } else {
                "打开串口"
            };
            if ui
                .add_sized([btn_w, 30.0], theme::primary_widget(label))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("打开/关闭当前配置的串口")
                .clicked()
            {
                if self.session_connected {
                    self.session.send(Command::Close);
                } else {
                    self.open_port();
                }
            }
        });
    }

    /// 布局2-子1：接收功能设置行（横向分布、高度固定、宽度自适应、无边框）。
    pub fn receive_settings_row(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new("显示").color(theme::TEXT_SOFT));
            widgets::combo(
                ui,
                80.0,
                26.0,
                true,
                self.config.display_mode.label(),
                "接收数据的显示格式（文本/HEX）",
                |ui| {
                    if ui
                        .selectable_value(&mut self.config.display_mode, DisplayMode::Text, "文本")
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
            ui.label(egui::RichText::new("编码").color(theme::TEXT_SOFT));
            widgets::combo(
                ui,
                86.0,
                26.0,
                true,
                self.config.text_encoding.label(),
                "文本解码使用的字符编码",
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
            ui.checkbox(&mut self.config.autoscroll, "自动滚动")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("数据自动滚动到底部");
            ui.checkbox(&mut self.paused, "暂停接收")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("暂停刷新展示区，统计与日志照常记录");
            ui.checkbox(&mut self.config.auto_refresh_ports, "自动刷新")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("每 5 秒自动检测串口列表");
            ui.checkbox(&mut self.config.show_sent_data, "显示发送")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("在接收区显示发送的数据（带 [TX] 标记）");

            // 记录日志：勾选后自动弹出文件保存对话框选择日志路径
            let log_resp = ui
                .checkbox(&mut self.log_on, "记录日志")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("勾选后自动选择日志文件路径并开始记录");
            if log_resp.changed() {
                if self.log_on {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("日志", &["log", "txt"])
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
                    egui::Button::new("打开日志路径")
                        .min_size(egui::vec2(88.0, 32.0)),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(if self.log_on {
                    "在系统文件管理器中定位日志文件"
                } else {
                    "请先勾选“记录日志”"
                })
                .clicked()
            {
                open_log_folder_path(log_path.as_deref());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized([72.0, 32.0], theme::secondary_widget("清空统计"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("RX/TX 字节计数归零")
                    .clicked()
                {
                    self.rx_total = 0;
                    self.tx_total = 0;
                }
                if ui
                    .add_sized([72.0, 32.0], theme::secondary_widget("清空显示"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("清空数据展示区")
                    .clicked()
                {
                    self.clear_display();
                }
            });
        });
    }

    /// 文件发送行。
    pub fn file_panel(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if ui
                .add_sized([102.0, 33.0], theme::primary_widget("发送文件"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("按当前模式发送所选文件")
                .clicked()
            {
                self.start_file_send();
            }
            // 统一样式下拉框：宽度与“发送文件”按钮一致（102px），高度相同（33px）
            widgets::combo(
                ui,
                102.0,
                33.0,
                true,
                self.config.file_mode.label(),
                "选择文件发送模式",
                |ui| {
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::WholeFile,
                        "整块发送",
                    );
                    ui.selectable_value(
                        &mut self.config.file_mode,
                        FileSendMode::LineByLine,
                        "逐行发送",
                    );
                },
            );
            // 逐行发送时：行间隔控件紧跟在模式下拉框后面
            if self.config.file_mode == FileSendMode::LineByLine {
                ui.label(egui::RichText::new("行间隔").color(theme::TEXT_SOFT));
                let resp = ui.add_sized(
                    [80.0, 24.0],
                    egui::DragValue::new(&mut self.config.file_line_interval_ms)
                        .range(0..=60_000)
                        .suffix(" ms"),
                );
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("逐行发送时相邻两行之间的等待时间（毫秒）");
            }
            if ui
                .add_sized([116.0, 33.0], theme::secondary_widget("选择文件"))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("选择要发送的文件")
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                self.pending_file = Some(path);
            }
            let name = self
                .pending_file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "未选择文件".to_string());
            // 文件名紧挨“选择文件”按钮，占满剩余宽度（超长截断），末尾保留“取消”按钮位置
            let spacing = ui.spacing().item_spacing.x;
            let cancel_w = if self.file_send_active { 72.0 + spacing } else { 0.0 };
            ui.add_sized(
                [(ui.available_width() - cancel_w).max(60.0), 30.0],
                egui::Label::new(egui::RichText::new(name).color(theme::TEXT_SOFT))
                    .halign(egui::Align::Min)
                    .truncate(),
            );
            if self.file_send_active
                && ui
                    .add_sized([72.0, 33.0], theme::primary_widget("取消"))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("取消本次文件发送")
                    .clicked()
            {
                self.session.send(Command::StopFile);
            }
        });
        if let Some((sent, total)) = self.file_progress {
            let frac = if total > 0 {
                sent as f32 / total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(ui.available_width())
                    .text(format!("{sent}/{total} 字节")),
            );
        }
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
            Err(e) => self.set_status(format!("枚举串口失败: {e}"), true),
        }
    }

    fn open_port(&mut self) {
        let port_name = self.config.port.port_name.trim().to_string();
        if port_name.is_empty() {
            self.set_status("请先选择串口".to_string(), true);
            return;
        }
        let baud: u32 = match self.baud_input.trim().parse() {
            Ok(v) if v > 0 => v,
            _ => {
                self.set_status("波特率无效".to_string(), true);
                return;
            }
        };
        self.config.port.baud_rate = baud;
        self.config.port.port_name = port_name;
        let settings = self.config.port.clone();
        self.session.send(Command::Open(settings));
        self.set_status("正在打开串口…".to_string(), false);
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
                    self.set_status(format!("无法创建日志文件: {e}"), true);
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

/// 标签 + 下拉框一行。
#[allow(clippy::too_many_arguments)]
fn combo_field(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    enabled: bool,
    selected: String,
    tip: &str,
    options: impl FnOnce(&mut egui::Ui),
) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.label(label).on_hover_text(tip);
        ui.add_enabled_ui(enabled, |ui| {
            widgets::combo(ui, width, 28.0, enabled, selected, tip, options);
        });
    });
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
