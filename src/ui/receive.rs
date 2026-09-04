// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 数据展示（接收区）：白色卡片带“接收区”标题 + 滚动展示区。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版）的接收区。
//!
//! 展示原理：原始接收/发送字节按批次拆分为“数据段”，标记文本（如 `[RX] `）
//! 与原始字节分离存储。只要遇到带时间戳的标记就先换行、再显示输出（每条数据段
//! 独占一行）。解码时原始字节流保持连续（流式解码器跨段保留未完成的多字节字符），
//! 标记文本插入到解码结果中——因此标记不会打断中文字符。

use crate::app::SerialApp;
use crate::codec;
use crate::config::{DisplayMode, TextEncoding};
use crate::serial::Command;
use crate::ui::theme;
use crate::ui::widgets;
use crate::util;
use eframe::egui;
use std::io::Write;
use std::path::PathBuf;

const DATA_DISPLAY_CAP: usize = 256 * 1024;
/// 显示缓存字符数上限（超出后丢弃最早的内容）
const DISPLAY_CACHE_CAP: usize = 300_000;
/// 终端显示缓冲字节数上限（超出后丢弃最早的内容并整体重建）
const TERMINAL_BUF_CAP: usize = 256 * 1024;
/// 终端显示文本字符数上限
const TERMINAL_TEXT_CAP: usize = 300_000;
/// 接收区右键菜单“全选”的一次性标记键（存储于 egui 临时数据）
const RECV_SELECT_ALL_KEY: &str = "recv_select_all";
/// ANSI 标准色（30–37 / 40–47）
const ANSI_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x7A, 0x7A, 0x7A), // 30 黑（深色背景下提亮以便可见）
    egui::Color32::from_rgb(0xCD, 0x31, 0x31), // 31 红
    egui::Color32::from_rgb(0x0D, 0xBC, 0x79), // 32 绿
    egui::Color32::from_rgb(0xE5, 0xE5, 0x10), // 33 黄
    egui::Color32::from_rgb(0x24, 0x72, 0xC8), // 34 蓝
    egui::Color32::from_rgb(0xBC, 0x3F, 0xBC), // 35 品红
    egui::Color32::from_rgb(0x11, 0xA8, 0xCD), // 36 青
    egui::Color32::from_rgb(0xE5, 0xE5, 0xE5), // 37 白
];
/// ANSI 亮色（90–97 / 100–107）
const ANSI_BRIGHT_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x66, 0x66, 0x66), // 90
    egui::Color32::from_rgb(0xF1, 0x4C, 0x4C), // 91
    egui::Color32::from_rgb(0x23, 0xD1, 0x8B), // 92
    egui::Color32::from_rgb(0xF5, 0xF5, 0x43), // 93
    egui::Color32::from_rgb(0x3B, 0x8E, 0xEA), // 94
    egui::Color32::from_rgb(0xD6, 0x70, 0xD6), // 95
    egui::Color32::from_rgb(0x29, 0xB8, 0xDB), // 96
    egui::Color32::from_rgb(0xFF, 0xFF, 0xFF), // 97
];

/// 一个数据段：标记文本 + 原始字节在展示缓冲中的范围。
#[derive(Clone, Debug)]
pub struct DisplaySeg {
    pub marker: String,
    pub start: usize,
    pub end: usize,
}

impl SerialApp {
    /// 接收设置行：横向流式自动换行、按内容自动尺寸。
    /// 依次为 显示/编码 两个独立实体、8 个复选框、打开日志路径按钮，
    /// 末尾为 清空显示 / 清空统计 按钮。
    pub fn receive_settings_row(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        const ROW_H: f32 = 26.0; // 下拉框高度
        const GAP: f32 = 8.0;
        const FIELD_INNER: f32 = 6.0;

        // 显示/编码 实体宽度 = label + 下拉框（按选中文本自适应）
        let label_font = egui::TextStyle::Body.resolve(ui.style());
        let measure_label = |t: &str| {
            ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(
                    t.to_owned(),
                    label_font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
            })
        };
        let combo_w = |text: &str| (measure_label(text) + 8.0 + 26.0 + 4.0).max(12.0);
        let display_label = self.config.display_mode.label(self.config.language);
        let display_combo_w = combo_w(display_label);
        let display_w = measure_label(s.display) + FIELD_INNER + display_combo_w;
        let encoding_label = self.config.text_encoding.label();
        let encoding_combo_w = combo_w(encoding_label);
        let encoding_w = measure_label(s.encoding) + FIELD_INNER + encoding_combo_w;

        ui.spacing_mut().item_spacing = egui::vec2(GAP, 4.0);
        ui.horizontal_wrapped(|ui| {
            // 显示：label + 下拉框 封装为一个独立实体
            ui.allocate_ui_with_layout(
                egui::vec2(display_w, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = FIELD_INNER;
                    ui.label(egui::RichText::new(s.display).color(theme::text_soft()));
                    widgets::combo(
                        ui,
                        display_combo_w,
                        ROW_H,
                        true,
                        display_label,
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
                },
            );

            // 编码：label + 下拉框 封装为一个独立实体
            ui.allocate_ui_with_layout(
                egui::vec2(encoding_w, ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = FIELD_INNER;
                    ui.label(egui::RichText::new(s.encoding).color(theme::text_soft()));
                    widgets::combo(
                        ui,
                        encoding_combo_w,
                        ROW_H,
                        true,
                        encoding_label,
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
                },
            );

            // 终端模式：切换时清空数据展示（普通展示区与终端缓冲互不残留）
            let terminal_resp = ui
                .checkbox(&mut self.config.terminal_mode, s.terminal_mode)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.terminal_mode_tip);
            if terminal_resp.changed() {
                self.clear_display();
            }
            let term_on = self.config.terminal_mode;
            // 回车发送：仅终端模式可用
            ui.add_enabled(
                term_on,
                egui::Checkbox::new(&mut self.config.terminal_enter_sends, s.enter_sends),
            )
            .on_hover_text(s.enter_sends_tip);
            // 本地回显：非终端模式显示发送数据、终端模式回显键盘输入，两种模式均有效
            ui.checkbox(&mut self.config.terminal_auto_echo, s.auto_echo)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.auto_echo_tip);
            ui.checkbox(&mut self.config.autoscroll, s.auto_scroll)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.auto_scroll_tip);
            // 显示时间戳：关闭后去掉 [HH:MM:SS.mmm] 前缀，仅保留 [RX]/[TX] 方向标记；
            // 终端模式下同样有效（勾选后终端显示也带时间戳）。
            let ts_resp = ui
                .checkbox(&mut self.config.show_timestamps, s.show_timestamps)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.show_timestamps_tip);
            if ts_resp.changed() {
                self.display_cache_invalid = true;
                self.display_dirty = true;
                self.terminal_cache_invalid = true;
                self.terminal_prompt = self.terminal_prefix_text();
            }
            ui.checkbox(&mut self.paused, s.pause_receive)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.pause_receive_tip);

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

            // 清空显示 / 清空统计：直接跟在前面控件之后按顺序排列
            if ui
                .add_sized([72.0, 32.0], theme::secondary_widget(s.clear_display))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.clear_display_tip)
                .clicked()
            {
                self.clear_display();
            }
            if ui
                .add_sized([72.0, 32.0], theme::secondary_widget(s.clear_stats))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.clear_stats_tip)
                .clicked()
            {
                self.rx_total = 0;
                self.tx_total = 0;
            }
        });
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

    /// 布局2-子2：接收区（内容填充、宽高自适应、无边框），只读文本框 + 符合只读样式的浅灰背景。
    pub fn receive_area(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        if self.config.terminal_mode {
            self.terminal_area(ui);
            return;
        }
        self.refresh_display_cache();

        // 右键菜单“全选”：egui 的 Label 选区没有公开的设置接口，且点击菜单项会触发
        // egui 自带的“点击别处取消选中”，因此用自定义高亮（蓝底白字）模拟选中效果。
        let select_all_id = egui::Id::new(RECV_SELECT_ALL_KEY);
        let mut select_all = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(select_all_id))
            .unwrap_or(false);
        if select_all && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.ctx().data_mut(|d| d.remove_temp::<bool>(select_all_id));
            select_all = false;
        }

        if self.display_dropped > 0 {
            ui.label(
                egui::RichText::new(s.fill(
                    s.display_dropped,
                    &[("n", self.display_dropped.to_string())],
                ))
                .small()
                .color(theme::text_soft()),
            );
        }
        // 只读展示区：无边框、深色背景（符合只读属性），不可编辑
        egui::Frame::new()
            .fill(theme::input_bg())
            .corner_radius(4)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                // 接收区支持横向与纵向自动滚动
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.config.autoscroll)
                    .show(ui, |ui| {
                        if self.display_cache.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(s.receive_empty).color(theme::text_soft()),
                                )
                                .halign(egui::Align::Min),
                            );
                        } else {
                            // 全选高亮与 egui 手动选区的配色一致（同一前景/背景色）
                            let sel_bg = ui.visuals().selection.bg_fill;
                            let sel_fg = ui.visuals().selection.stroke.color;
                            let label = egui::Label::new(
                                egui::RichText::new(&self.display_cache)
                                    .monospace()
                                    .color(if select_all {
                                        sel_fg
                                    } else {
                                        theme::text()
                                    }),
                            )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .halign(egui::Align::Min);
                            // 全选高亮：用选区色模拟整段选中
                            let resp = if select_all {
                                egui::Frame::new()
                                    .fill(sel_bg)
                                    .inner_margin(egui::Margin::ZERO)
                                    .show(ui, |ui| ui.add(label))
                                    .inner
                            } else {
                                ui.add(label)
                            };
                            // 点击/拖动该区域时退出全选高亮，恢复 egui 原生文本选择
                            if select_all && (resp.clicked() || resp.dragged()) {
                                ui.ctx().data_mut(|d| d.remove_temp::<bool>(select_all_id));
                            }
                            // 只读展示区右键菜单：全选 + 复制（无粘贴）
                            resp.context_menu(|ui| {
                                if ui.selectable_label(false, s.select_all).clicked() {
                                    ui.ctx().data_mut(|d| d.insert_temp(select_all_id, true));
                                    ui.close();
                                }
                                if ui.selectable_label(false, s.copy).clicked() {
                                    ui.ctx().copy_text(self.display_cache.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
            });
    }

    /// 终端模式接收区：深色背景、支持 ANSI 颜色（「显示时间戳」开启时同样
    /// 显示时间戳）；接收显示区本身就是输入区（类似 PuTTY/minicom），
    /// 点击后直接键盘输入，右键菜单支持全选/复制/粘贴。
    pub(crate) fn terminal_area(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        self.refresh_terminal_text();
        // shell 提示符：行头恒为 #（时间戳可选），两种发送模式都在新行开始时生成
        if self.terminal_prompt.is_empty() {
            self.terminal_prompt = self.terminal_prefix_text();
        }

        // 右键菜单「全选」：与普通接收区一致，用自定义高亮模拟选中
        let select_all_id = egui::Id::new(RECV_SELECT_ALL_KEY);
        let mut select_all = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(select_all_id))
            .unwrap_or(false);
        if select_all && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.ctx().data_mut(|d| d.remove_temp::<bool>(select_all_id));
            select_all = false;
        }

        let avail_h = ui.available_height().max(40.0);
        // 整个终端区域是一个可点击获得焦点的控件，键盘输入直接作用于该区域
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), avail_h),
            egui::Sense::click_and_drag(),
        );
        if resp.clicked() {
            resp.request_focus();
        }
        let resp_id = resp.id;
        let focused = resp.has_focus();
        if focused {
            // 终端区域自己消费方向键（历史浏览/光标移动），
            // 声明事件过滤器防止 egui 把 ↑/↓/←/→ 当作焦点导航导致失焦。
            ui.ctx().memory_mut(|mem| {
                mem.set_focus_lock_filter(
                    resp_id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
        }
        let blink_on = focused && (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;

        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            egui::Frame::new()
                .fill(theme::TERMINAL_BG)
                .corner_radius(4)
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .stick_to_bottom(self.config.autoscroll)
                        .show(ui, |ui| {
                            if !focused && self.terminal_text.is_empty() && self.terminal_input.is_empty()
                            {
                                ui.label(
                                    egui::RichText::new(s.terminal_empty)
                                        .monospace()
                                        .color(egui::Color32::from_gray(120)),
                                );
                            } else {
                                let label = egui::Label::new(self.terminal_job_with_input(ui, blink_on, select_all));
                                let label_resp = if select_all {
                                    egui::Frame::new()
                                        .fill(ui.visuals().selection.bg_fill)
                                        .inner_margin(egui::Margin::ZERO)
                                        .show(ui, |ui| ui.add(label))
                                        .inner
                                } else {
                                    ui.add(label)
                                };
                                if select_all && (label_resp.clicked() || label_resp.dragged()) {
                                    ui.ctx().data_mut(|d| d.remove_temp::<bool>(select_all_id));
                                }
                                // 右键菜单挂在文本上（与普通接收区一致）：全选/复制/粘贴
                                label_resp.context_menu(|ui| {
                                    self.terminal_menu(ui, select_all_id, resp_id);
                                });
                            }
                        });
                });
        });

        // 空白区域右键同样弹出菜单
        resp.context_menu(|ui| self.terminal_menu(ui, select_all_id, resp_id));

        // 仅当终端区域拥有键盘焦点时处理输入事件
        if focused {
            self.handle_terminal_keys(ui);
        }
    }

    /// 终端右键菜单：全选 / 复制 / 粘贴（粘贴通过 RequestPaste 进入键盘处理）。
    fn terminal_menu(
        &self,
        ui: &mut egui::Ui,
        select_all_id: egui::Id,
        resp_id: egui::Id,
    ) {
        let s = self.t();
        if ui.selectable_label(false, s.select_all).clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(select_all_id, true));
            ui.close();
        }
        if ui.selectable_label(false, s.copy).clicked() {
            ui.ctx().copy_text(self.terminal_text.clone());
            ui.close();
        }
        if ui.selectable_label(false, s.paste).clicked() {
            // 请求焦点后让终端区域接收粘贴事件（handle_terminal_keys 处理）
            ui.ctx().memory_mut(|mem| mem.request_focus(resp_id));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
    }

    /// 终端显示内容：ANSI 文本 + 待发送输入行 + 闪烁块状光标。
    fn terminal_job_with_input(
        &self,
        ui: &egui::Ui,
        blink_on: bool,
        selected: bool,
    ) -> egui::text::LayoutJob {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        // 数据段用白色/ANSI 颜色，时间戳标记统一用绿色（仿 shell 提示符）
        let mut cursor = 0usize;
        for &(start, end) in &self.terminal_marker_spans {
            if start > cursor {
                append_ansi_text(&mut job, &font_id, &self.terminal_text[cursor..start]);
            }
            push_run(
                &mut job,
                &self.terminal_text[start..end],
                &font_id,
                theme::TERMINAL_PROMPT,
                egui::Color32::TRANSPARENT,
            );
            cursor = end;
        }
        if cursor < self.terminal_text.len() {
            append_ansi_text(&mut job, &font_id, &self.terminal_text[cursor..]);
        }
        let chars: Vec<char> = self.terminal_input.chars().collect();
        let cursor = self.terminal_cursor.min(chars.len());
        let before: String = chars[..cursor].iter().collect();
        let after: String = chars[cursor..].iter().collect();
        // shell 提示符：输入行以 时间戳（开启）+绿色 #（恒有）为行头。
        // 回车发送模式下输入行是虚拟行（缓冲未发送）；即时发送模式下，
        // 仅当显示处于新行行首时补提示符（按下回车已结束上一行）。
        if self.config.terminal_enter_sends {
            if !self.terminal_text.is_empty()
                && !self.terminal_text.ends_with('\n')
                && !self.terminal_text.ends_with('\r')
            {
                // 提示符总在新一行行首
                job.append(
                    "\n",
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color: theme::TERMINAL_TEXT,
                        ..Default::default()
                    },
                );
            }
            job.append(
                &self.terminal_prompt,
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: theme::TERMINAL_PROMPT,
                    ..Default::default()
                },
            );
        } else if self.terminal_text.is_empty()
            || self.terminal_text.ends_with('\n')
            || self.terminal_text.ends_with('\r')
        {
            job.append(
                &self.terminal_prompt,
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: theme::TERMINAL_PROMPT,
                    ..Default::default()
                },
            );
        }
        if !before.is_empty() {
            job.append(
                &before,
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: theme::TERMINAL_TEXT,
                    ..Default::default()
                },
            );
        }
        if blink_on {
            // 块状光标：以提示色填充一个空格
            job.append(
                " ",
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: theme::TERMINAL_PROMPT,
                    background: theme::TERMINAL_PROMPT,
                    ..Default::default()
                },
            );
        }
        if !after.is_empty() {
            job.append(
                &after,
                0.0,
                egui::TextFormat {
                    font_id,
                    color: theme::TERMINAL_TEXT,
                    ..Default::default()
                },
            );
        }
        // 右键全选：前景色与手动选择一致（用 egui 选区的文字色）
        if selected {
            let fg = ui.visuals().selection.stroke.color;
            for sec in &mut job.sections {
                sec.format.color = fg;
                sec.format.background = egui::Color32::TRANSPARENT;
            }
        }
        job
    }

    /// 终端键盘输入处理：
    /// - 回车发送开启：字符进入待发送输入行，回车时发送整行（回车字符 CR 一并发送）；
    /// - 回车发送关闭：按键即发，退格发 DEL，回车发 CR。
    fn handle_terminal_keys(&mut self, ui: &egui::Ui) {
        let events: Vec<egui::Event> = ui.input(|i| i.events.clone());
        let enter_sends = self.config.terminal_enter_sends;
        for ev in events {
            match ev {
                // 快捷键：全选 / 复制 / 粘贴（与右键菜单一致）
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.command => match key {
                    egui::Key::A => {
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(egui::Id::new(RECV_SELECT_ALL_KEY), true));
                    }
                    egui::Key::C => {
                        ui.ctx().copy_text(self.terminal_text.clone());
                    }
                    egui::Key::V => {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    }
                    _ => {}
                },
                egui::Event::Text(text) | egui::Event::Paste(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    if enter_sends {
                        self.terminal_input_insert(&text);
                    } else {
                        let bytes = codec::text::encode(&text, self.config.text_encoding);
                        self.terminal_send_bytes(&bytes);
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    ..
                } => {
                    if enter_sends {
                        let line = std::mem::take(&mut self.terminal_input);
                        self.terminal_cursor = 0;
                        // 发送后退出历史浏览状态，避免旧位置/草稿残留
                        self.terminal_history_index = None;
                        self.terminal_history_draft.clear();
                        self.terminal_send_line(&line);
                    } else {
                        // 即时发送：回车只发送 CR 给串口；本地显示仿 shell 换行，
                        // 使下一行行首出现 #（不再原样回显不可见的 CR）
                        self.session.send(Command::Write(b"\r".to_vec()));
                        if self.config.terminal_auto_echo {
                            self.append_terminal_bytes(b"\n");
                        }
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    if enter_sends {
                        self.terminal_input_backspace();
                    } else {
                        self.terminal_send_bytes(b"\x7F");
                    }
                }
                egui::Event::Key {
                    key: egui::Key::Delete,
                    pressed: true,
                    ..
                } => {
                    if enter_sends {
                        self.terminal_input_delete();
                    }
                }
                egui::Event::Key {
                    key: egui::Key::ArrowLeft,
                    pressed: true,
                    ..
                } if enter_sends => {
                    self.terminal_cursor = self.terminal_cursor.saturating_sub(1);
                    cancel_focus_navigation(ui);
                }
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    pressed: true,
                    ..
                } if enter_sends => {
                    let n = self.terminal_input.chars().count();
                    self.terminal_cursor = (self.terminal_cursor + 1).min(n);
                    cancel_focus_navigation(ui);
                }
                egui::Event::Key {
                    key: egui::Key::ArrowUp,
                    pressed: true,
                    ..
                } if enter_sends => {
                    self.terminal_history_older();
                    cancel_focus_navigation(ui);
                }
                egui::Event::Key {
                    key: egui::Key::ArrowDown,
                    pressed: true,
                    ..
                } if enter_sends => {
                    self.terminal_history_newer();
                    cancel_focus_navigation(ui);
                }
                egui::Event::Key {
                    key: egui::Key::Home,
                    pressed: true,
                    ..
                } if enter_sends => {
                    self.terminal_cursor = 0;
                }
                egui::Event::Key {
                    key: egui::Key::End,
                    pressed: true,
                    ..
                } if enter_sends => {
                    self.terminal_cursor = self.terminal_input.chars().count();
                }
                _ => {}
            }
        }
    }

    /// 在待发送输入行的光标处插入文本。
    fn terminal_input_insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let chars: Vec<char> = self.terminal_input.chars().collect();
        let idx = self.terminal_cursor.min(chars.len());
        let mut out = String::with_capacity(self.terminal_input.len() + text.len());
        out.extend(chars[..idx].iter());
        out.push_str(text);
        out.extend(chars[idx..].iter());
        self.terminal_input = out;
        self.terminal_cursor = idx + text.chars().count();
    }

    /// 退格：删除待发送输入行中光标前的字符。
    fn terminal_input_backspace(&mut self) {
        let chars: Vec<char> = self.terminal_input.chars().collect();
        if self.terminal_cursor == 0 || chars.is_empty() {
            return;
        }
        let idx = self.terminal_cursor.min(chars.len());
        self.terminal_input = chars[..idx - 1]
            .iter()
            .chain(chars[idx..].iter())
            .collect();
        self.terminal_cursor = idx - 1;
    }

    /// Delete：删除待发送输入行中光标处的字符。
    fn terminal_input_delete(&mut self) {
        let chars: Vec<char> = self.terminal_input.chars().collect();
        if self.terminal_cursor >= chars.len() {
            return;
        }
        let idx = self.terminal_cursor;
        self.terminal_input = chars[..idx].iter().chain(chars[idx + 1..].iter()).collect();
    }

    /// ↑ 键：在发送历史中向上翻（越新的命令下标越小）；首次按下保存当前草稿。
    fn terminal_history_older(&mut self) {
        if self.send_history.is_empty() {
            return;
        }
        match self.terminal_history_index {
            None => {
                // 第一次按 ↑：保存当前输入行，跳到最近一条命令
                self.terminal_history_draft = std::mem::take(&mut self.terminal_input);
                self.terminal_history_index = Some(0);
            }
            Some(i) if i + 1 < self.send_history.len() => {
                self.terminal_history_index = Some(i + 1);
            }
            _ => return, // 已是最旧一条
        }
        let i = self.terminal_history_index.expect("刚设置过浏览位置");
        self.terminal_input = self.send_history[i].clone();
        self.terminal_cursor = self.terminal_input.chars().count();
    }

    /// ↓ 键：向较新的历史翻；越过最近一条后恢复浏览前的草稿。
    fn terminal_history_newer(&mut self) {
        match self.terminal_history_index {
            None => {}
            Some(0) => {
                self.terminal_history_index = None;
                self.terminal_input = std::mem::take(&mut self.terminal_history_draft);
                self.terminal_cursor = self.terminal_input.chars().count();
            }
            Some(i) => {
                let i = i - 1;
                self.terminal_history_index = Some(i);
                self.terminal_input = self.send_history[i].clone();
                self.terminal_cursor = self.terminal_input.chars().count();
            }
        }
    }

    /// 发送终端输入的整行内容（行尾附 CR），并按「自动回显」决定是否本地显示。
    fn terminal_send_line(&mut self, line: &str) {
        // 回车发送的命令行记入发送区历史下拉框（去重置顶、忽略空行）
        self.record_send_history(line);
        let line_bytes = codec::text::encode(line, self.config.text_encoding);
        let mut bytes = line_bytes.clone();
        bytes.push(b'\r');
        self.session.send(Command::Write(bytes));
        // 本地回显：命令内容 + 一个真正的换行（不回显 CR）。
        // 这样回车后光标位于下一行行首，随后对端返回的数据显示在第二行，
        // 而不是被追加到当前命令行末尾。
        if self.config.terminal_auto_echo {
            self.append_terminal_bytes(&line_bytes);
            self.append_terminal_bytes(b"\n");
        }
        // 回车发送后开启新一行：刷新行头提示符（时间戳或 #）
        if self.config.terminal_enter_sends {
            self.terminal_prompt = self.terminal_prefix_text();
        }
    }

    /// 发送终端输入字节；开启「自动回显」时同步追加到终端显示。
    fn terminal_send_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.session.send(Command::Write(bytes.to_vec()));
        if self.config.terminal_auto_echo {
            self.append_terminal_bytes(bytes);
        }
    }

    /// 行头前缀：行头恒为绿色 `#`；开启时间戳时在其前加 `[时间戳]`，
    /// 关闭时仅显示 `#`（“显示时间戳”只控制是否添加时间戳）。
    fn terminal_prefix_text(&self) -> String {
        if self.config.show_timestamps {
            format!("[{}]# ", util::now_hms_ms())
        } else {
            "# ".to_string()
        }
    }

    /// 追加字节到终端显示缓冲（仅接收数据与本地回显使用）。
    /// 前缀不带 [RX]/[TX] 方向标记：终端仿照 shell，以时间戳或 # 作提示符。
    pub(crate) fn append_terminal_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // 原始字节原样追加，不因“一批数据”而强制换行；
        // 行首前缀由解码阶段按真实换行符（0x0A）生成。
        self.terminal_buffer.extend_from_slice(bytes);
        if self.terminal_buffer.len() > TERMINAL_BUF_CAP {
            let cut = self.terminal_buffer.len() - TERMINAL_BUF_CAP;
            self.terminal_buffer.drain(..cut);
            // 丢弃头部后流式解码状态失效，需要整体重建
            self.terminal_cache_invalid = true;
        }
        self.terminal_decoded_len = self
            .terminal_decoded_len
            .min(self.terminal_buffer.len());
    }

    /// 增量解码终端缓冲到显示文本；编码/显示模式变化或缓冲被裁剪时整体重建。
    fn refresh_terminal_text(&mut self) {
        if self.terminal_cache_invalid {
            self.terminal_cache_invalid = false;
            self.terminal_text.clear();
            self.terminal_marker_spans.clear();
            self.terminal_decoder = None;
            self.terminal_decoder_enc = None;
            self.terminal_decoded_len = 0;
        }
        if self.terminal_decoded_len >= self.terminal_buffer.len() {
            return;
        }
        // 行首前缀：时间戳开启时为 [HH:MM:SS.mmm] ，关闭时为绿色 #。
        // 只在真正开始一行数据（文本为空或上一字符是换行/CR）时插入，不强制换行；
        // 数据中间的换行保持原样，换行后的下一行行首再补前缀。
        let marker = self.terminal_prefix_text();
        let mut line_start = self.terminal_text.is_empty()
            || self.terminal_text.ends_with('\n')
            || self.terminal_text.ends_with('\r');
        let bytes = &self.terminal_buffer[self.terminal_decoded_len..];
        let decoded: String = match self.config.display_mode {
            DisplayMode::Text => {
                let enc = self.config.text_encoding;
                if enc == TextEncoding::Ascii {
                    codec::text::decode(bytes, enc)
                } else {
                    if self.terminal_decoder_enc != Some(enc) {
                        self.terminal_decoder = Some(match enc {
                            TextEncoding::Utf8 => encoding_rs::UTF_8.new_decoder(),
                            TextEncoding::Gbk => encoding_rs::GBK.new_decoder(),
                            TextEncoding::Ascii => unreachable!(),
                        });
                        self.terminal_decoder_enc = Some(enc);
                    }
                    if let Some(dec) = self.terminal_decoder.as_mut() {
                        stream_decode(dec, bytes, false)
                    } else {
                        String::new()
                    }
                }
            }
            DisplayMode::Hex => {
                let mut hex = codec::hex::format_hex(bytes);
                hex.push('\n');
                hex
            }
        };
        for ch in decoded.chars() {
            if line_start && ch != '\n' && ch != '\r' {
                let start = self.terminal_text.len();
                self.terminal_text.push_str(&marker);
                self.terminal_marker_spans.push((start, self.terminal_text.len()));
                line_start = false;
            }
            self.terminal_text.push(ch);
            if ch == '\n' {
                line_start = true;
            } else if ch == '\r' {
                // 渲染层把孤立 CR 当换行显示，因此 CR 后同样视为行首
                line_start = true;
            }
        }
        self.terminal_decoded_len = self.terminal_buffer.len();
        if self.terminal_text.len() > TERMINAL_TEXT_CAP {
            let cut =
                self.terminal_text
                    .floor_char_boundary(self.terminal_text.len() - TERMINAL_TEXT_CAP);
            self.terminal_text.drain(..cut);
            self.terminal_marker_spans = self
                .terminal_marker_spans
                .iter()
                .filter_map(|&(s, e)| {
                    if e <= cut {
                        None
                    } else {
                        Some((s.saturating_sub(cut), e - cut))
                    }
                })
                .collect();
        }
    }

    /// 清空终端显示缓冲（进入终端模式或点击「清空显示」时调用）。
    pub(crate) fn clear_terminal_display(&mut self) {
        self.terminal_buffer.clear();
        self.terminal_text.clear();
        self.terminal_marker_spans.clear();
        self.terminal_prompt.clear();
        self.terminal_decoder = None;
        self.terminal_decoder_enc = None;
        self.terminal_cache_invalid = false;
        self.terminal_decoded_len = 0;
        self.terminal_history_index = None;
        self.terminal_history_draft.clear();
    }

    /// 增量更新显示缓存：仅解码新增的数据段。
    fn refresh_display_cache(&mut self) {
        if !self.display_dirty {
            return;
        }
        self.display_dirty = false;

        if self.display_cache_invalid {
            self.display_cache_invalid = false;
            self.display_segments_decoded = 0;
            self.display_cache.clear();
            self.display_decoder = None;
            self.display_decoder_enc = None;
        }

        let seg_count = self.display_segments.len();
        if self.display_segments_decoded == 0 && !self.display_segments.is_empty() {
            // 首次或失效后：全量重建（克隆段数据以绕开借用冲突）
            let segs: Vec<(usize, usize, String)> = self
                .display_segments
                .iter()
                .map(|s| (s.start, s.end, s.marker.clone()))
                .collect();
            for (start, end, marker) in segs {
                self.append_segment_to_cache(start, end, &marker);
            }
            self.display_segments_decoded = seg_count;
        } else if self.display_segments_decoded < seg_count {
            let segs: Vec<(usize, usize, String)> = self.display_segments
                [self.display_segments_decoded..]
                .iter()
                .map(|s| (s.start, s.end, s.marker.clone()))
                .collect();
            for (start, end, marker) in segs {
                self.append_segment_to_cache(start, end, &marker);
            }
            self.display_segments_decoded = seg_count;
        }

        if self.display_cache.len() > DISPLAY_CACHE_CAP {
            let cut =
                self.display_cache
                    .floor_char_boundary(self.display_cache.len() - DISPLAY_CACHE_CAP);
            self.display_cache.drain(..cut);
        }
    }

    /// 将某数据段（标记 + 字节）解码并追加到显示缓存。
    fn append_segment_to_cache(&mut self, start: usize, end: usize, marker: &str) {
        let bytes = &self.data_display[start..end];
        // 只要遇到时间戳（RX/TX 标记）就先换行再显示输出；
        // 仅当展示区还是空的时候不补，避免首行出现空行。
        if !self.display_cache.is_empty() {
            self.display_cache.push('\n');
        }
        // 关闭「显示时间戳」时去掉 [HH:MM:SS.mmm] 前缀，仅保留 [RX]/[TX] 方向标记
        if self.config.show_timestamps {
            self.display_cache.push_str(marker);
        } else {
            self.display_cache.push_str(marker_without_timestamp(marker));
        }
        match self.config.display_mode {
            DisplayMode::Text => {
                let enc = self.config.text_encoding;
                if enc == TextEncoding::Ascii {
                    self.display_cache.push_str(&codec::text::decode(bytes, enc));
                } else {
                    if self.display_decoder_enc != Some(enc) {
                        self.display_decoder = Some(match enc {
                            TextEncoding::Utf8 => encoding_rs::UTF_8.new_decoder(),
                            TextEncoding::Gbk => encoding_rs::GBK.new_decoder(),
                            TextEncoding::Ascii => unreachable!(),
                        });
                        self.display_decoder_enc = Some(enc);
                    }
                    if let Some(dec) = self.display_decoder.as_mut() {
                        // 流式解码：跨数据段的多字节字符由解码器内部保留
                        self.display_cache
                            .push_str(&stream_decode(dec, bytes, false));
                    }
                }
            }
            DisplayMode::Hex => {
                self.display_cache.push_str(&codec::hex::format_hex(bytes));
            }
        }
    }

    /// 追加接收数据段；每条带 `[HH:MM:SS.mmm] [RX]` 时间戳与方向标记。
    pub(crate) fn append_rx(&mut self, bytes: &[u8]) {
        let marker = format!("[{}] [RX] ", util::now_hms_ms());
        append_segment(
            &mut self.data_display,
            &mut self.display_segments,
            self.paused,
            &mut self.display_dropped,
            &mut self.display_dirty,
            &mut self.display_cache_invalid,
            marker,
            bytes,
        );
        // 终端模式显示同样的原始字节（无时间戳、无标记），暂停时同样丢弃
        if !self.paused {
            self.append_terminal_bytes(bytes);
        }
    }

    /// 追加发送数据段（仅当「显示发送」开启时由事件处理调用），每条带 `[HH:MM:SS.mmm] [TX]` 时间戳与方向标记。
    pub(crate) fn append_tx(&mut self, bytes: &[u8]) {
        let marker = format!("[{}] [TX] ", util::now_hms_ms());
        append_segment(
            &mut self.data_display,
            &mut self.display_segments,
            self.paused,
            &mut self.display_dropped,
            &mut self.display_dirty,
            &mut self.display_cache_invalid,
            marker,
            bytes,
        );
    }

    /// 清空展示缓冲。
    pub(crate) fn clear_display(&mut self) {
        self.data_display.clear();
        self.display_segments.clear();
        self.display_segments_decoded = 0;
        self.display_cache.clear();
        self.display_dropped = 0;
        self.display_cache_invalid = false;
        self.display_decoder = None;
        self.display_decoder_enc = None;
        self.display_dirty = false;
        self.clear_terminal_display();
    }

    /// 写入日志：仅记录收到的原始数据，不增加时间戳与 [TX]/[RX] 标记。
    pub(crate) fn log_write_rx(&mut self, bytes: &[u8]) {
        if !self.log_on {
            return;
        }
        let Some(f) = self.log_file.as_mut() else {
            return;
        };
        let text = codec::text::decode(bytes, self.config.text_encoding);
        let _ = f.write_all(text.as_bytes());
        let _ = f.write_all(b"\n");
        let _ = f.flush();
    }
}

/// 在系统文件管理器中打开日志文件所在目录（Windows 定位到文件，其他平台打开目录）。
/// 方向键已被终端消费时，清掉 egui 本帧的“按方向键移动焦点”导航意图，
/// 否则帧末会把焦点移给相邻控件（导致第二次按键失效）。
fn cancel_focus_navigation(ui: &egui::Ui) {
    ui.ctx()
        .memory_mut(|mem| mem.move_focus(egui::FocusDirection::None));
}

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

/// 追加一个数据段；缓冲超过上限 2 倍时裁剪到上限并标记缓存失效。
#[allow(clippy::too_many_arguments)]
fn append_segment(
    buf: &mut Vec<u8>,
    segments: &mut Vec<DisplaySeg>,
    paused: bool,
    dropped: &mut u64,
    dirty: &mut bool,
    invalid: &mut bool,
    marker: String,
    bytes: &[u8],
) {
    if paused {
        return;
    }
    let start = buf.len();
    buf.extend_from_slice(bytes);
    segments.push(DisplaySeg {
        marker,
        start,
        end: buf.len(),
    });
    if buf.len() > DATA_DISPLAY_CAP * 2 {
        let drop = buf.len() - DATA_DISPLAY_CAP;
        buf.drain(0..drop);
        *dropped += drop as u64;
        *invalid = true;
        for seg in segments.iter_mut() {
            if seg.end <= drop {
                seg.end = 0;
                seg.start = 0;
            } else if seg.start < drop {
                seg.start = 0;
                seg.end -= drop;
            } else {
                seg.start -= drop;
                seg.end -= drop;
            }
        }
        segments.retain(|s| s.end > s.start);
    }
    *dirty = true;
}

/// 去掉标记中的 `[HH:MM:SS.mmm]` 时间戳前缀，保留 `[RX]`/`[TX]` 方向标记。
fn marker_without_timestamp(marker: &str) -> &str {
    match marker.find("] [") {
        Some(i) => &marker[i + 2..],
        None => marker,
    }
}

/// 用流式解码器解码字节并返回 UTF-8 字符串。
/// 使用足够大的输出缓冲，避免 `decode_to_string` 因 String 容量不足而截断。
fn stream_decode(dec: &mut encoding_rs::Decoder, bytes: &[u8], last: bool) -> String {
    let mut out = vec![0u8; bytes.len() * 3 + 16];
    let (_, read, written, _) = dec.decode_to_utf8(bytes, &mut out, last);
    debug_assert!(read == bytes.len(), "解码输出缓冲不足");
    String::from_utf8_lossy(&out[..written]).into_owned()
}

/// 将终端文本（含 ANSI SGR 颜色转义）构建为带颜色的 LayoutJob。
#[cfg(test)]
fn terminal_job(font_id: &egui::FontId, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    append_ansi_text(&mut job, font_id, text);
    job
}

/// 解析一段终端文本（含 ANSI SGR 颜色转义）并把文本段追加到已有 job。
/// 标记段与数据段分开追加时，各自以默认前景色开始解析。
fn append_ansi_text(job: &mut egui::text::LayoutJob, font_id: &egui::FontId, text: &str) {
    let mut fg = theme::TERMINAL_TEXT;
    let mut bg = egui::Color32::TRANSPARENT;
    let mut bold = false;
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\u{1b}' {
            push_run(job, &buf, font_id, fg, bg);
            buf.clear();
            if i + 1 < chars.len() && chars[i + 1] == '[' {
                // CSI 序列：`ESC [ ... <final>`，final 字节为 0x40–0x7E
                let mut j = i + 2;
                while j < chars.len() && !(chars[j] as u32 >= 0x40 && chars[j] as u32 <= 0x7E) {
                    j += 1;
                }
                if j < chars.len() && chars[j] == 'm' {
                    let params: String = chars[i + 2..j].iter().collect();
                    apply_sgr(&params, &mut fg, &mut bg, &mut bold);
                }
                i = j + 1;
            } else if i + 1 < chars.len() {
                // 两字符转义序列（如 `ESC ( B`）直接丢弃
                i += 2;
            } else {
                i += 1;
            }
        } else {
            match ch {
                // 终端常见的 CR/CRLF 都只产生一次换行：
                // CR 转成换行；若后面紧跟 LF（CRLF）则把 LF 一并消费
                '\r' => {
                    buf.push('\n');
                    if i + 1 < chars.len() && chars[i + 1] == '\n' {
                        i += 1;
                    }
                }
                '\n' | '\t' => buf.push(ch),
                c if (c as u32) < 0x20 => {} // 其余控制字符不显示
                _ => buf.push(ch),
            }
            i += 1;
        }
    }
    push_run(job, &buf, font_id, fg, bg);
}

fn push_run(
    job: &mut egui::text::LayoutJob,
    text: &str,
    font_id: &egui::FontId,
    fg: egui::Color32,
    bg: egui::Color32,
) {
    if text.is_empty() {
        return;
    }
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: fg,
            background: bg,
            ..Default::default()
        },
    );
}

/// 解析 SGR 参数并更新当前前景/背景色与加粗状态。
fn apply_sgr(
    params: &str,
    fg: &mut egui::Color32,
    bg: &mut egui::Color32,
    bold: &mut bool,
) {
    for p in params.split(';') {
        match p {
            "" | "0" => {
                *fg = theme::TERMINAL_TEXT;
                *bg = egui::Color32::TRANSPARENT;
                *bold = false;
            }
            "1" => *bold = true,
            "22" => *bold = false,
            "39" => *fg = theme::TERMINAL_TEXT,
            "49" => *bg = egui::Color32::TRANSPARENT,
            _ => {
                if let Ok(n) = p.parse::<u16>() {
                    match n {
                        30..=37 => {
                            *fg = if *bold {
                                ANSI_BRIGHT_COLORS[(n - 30) as usize]
                            } else {
                                ANSI_COLORS[(n - 30) as usize]
                            }
                        }
                        90..=97 => *fg = ANSI_BRIGHT_COLORS[(n - 90) as usize],
                        40..=47 => *bg = ANSI_COLORS[(n - 40) as usize],
                        100..=107 => *bg = ANSI_BRIGHT_COLORS[(n - 100) as usize],
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{SerialApp, UiPanel};
    use crate::config::Config;
    use crate::i18n::Language;
    use crate::serial::SerialSession;
    use egui_dock::DockState;
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
            terminal_buffer: Vec::new(),
            terminal_text: String::new(),
            terminal_decoder: None,
            terminal_decoder_enc: None,
            terminal_cache_invalid: false,
            terminal_decoded_len: 0,
            terminal_marker_spans: Vec::new(),
            terminal_prompt: String::new(),
            terminal_input: String::new(),
            terminal_cursor: 0,
            terminal_history_index: None,
            terminal_history_draft: String::new(),
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
            dock_state: Some(DockState::new(UiPanel::ALL.to_vec())),
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
    fn utf8_char_split_across_batches_not_corrupted() {
        // “中” 的 UTF-8 为 E4 B8 AD，分两批到达；标记插入在解码结果中，不打断字符
        let mut app = make_app();
        app.append_rx(&[0xE4, 0xB8]);
        app.refresh_display_cache();
        app.append_rx(&[0xAD, 0x21]); // 补充字节 + '!'
        app.refresh_display_cache();
        assert!(app.display_cache.starts_with('['));
        assert!(app.display_cache.contains("[RX] 中!"));
    }

    #[test]
    fn display_cache_omits_timestamps_when_disabled() {
        // 关闭「显示时间戳」后，缓存只保留 [RX]/[TX] 方向标记，不带 [HH:MM:SS.mmm] 前缀
        let mut app = make_app();
        app.config.show_timestamps = false;
        app.append_rx(b"hi");
        app.refresh_display_cache();
        assert!(app.display_cache.starts_with("[RX] hi"));
        assert!(!app.display_cache.contains("] ["));
    }

    #[test]
    fn mode_switch_rebuilds_cache() {
        let mut app = make_app();
        app.append_rx(&[0x01, 0x0A, 0xFF]);
        app.refresh_display_cache();
        assert!(app.display_cache.ends_with("\u{1}\n\u{FFFD}"));

        app.config.display_mode = DisplayMode::Hex;
        app.display_cache_invalid = true;
        app.display_dirty = true;
        app.refresh_display_cache();
        assert!(app.display_cache.ends_with("01 0A FF"));
    }

    #[test]
    fn buffer_trim_invalidates_and_keeps_tail() {
        let mut app = make_app();
        // 用超大数据量触发裁剪（DATA_DISPLAY_CAP = 256KB，超 2 倍才裁剪）
        let chunk = vec![b'A'; 4096];
        for _ in 0..(DATA_DISPLAY_CAP * 2 / 4096 + 2) {
            app.append_rx(&chunk);
        }
        assert!(app.display_cache_invalid);
        assert!(app.data_display.len() <= DATA_DISPLAY_CAP + 16 * 1024);
        app.refresh_display_cache();
        assert!(app.display_cache.len() <= DATA_DISPLAY_CAP + 16 * 1024);
        assert!(app.display_cache.ends_with('A'));
    }

    #[test]
    fn rx_tx_segments_are_separated_by_newline() {
        let mut app = make_app();
        app.append_rx(b"hello");
        app.append_tx(b"world");
        app.refresh_display_cache();
        let lines: Vec<&str> = app.display_cache.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("[RX] hello"));
        assert!(lines[1].contains("[TX] world"));
    }

    #[test]
    fn timestamp_always_preceded_by_newline() {
        let mut app = make_app();
        app.append_rx(b"ok\n"); // 上一段数据已以换行结尾
        app.append_tx(b"next");
        app.refresh_display_cache();
        // 只要遇到时间戳就先换行：即使上一段已换行也会再补一个，形成空行
        let lines: Vec<&str> = app.display_cache.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("[RX] ok"));
        assert!(lines[1].is_empty());
        assert!(lines[2].contains("[TX] next"));
    }

    #[test]
    fn receive_area_select_all_renders_highlight() {
        let mut app = make_app();
        app.append_rx(b"hello");
        app.refresh_display_cache();
        let ctx = egui::Context::default();
        // 模拟右键菜单点击“全选”后设置的标记
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(RECV_SELECT_ALL_KEY), true));

        let output = run_ui(&ctx, Default::default(), |ui| {
            app.receive_area(ui);
        });

        // 全选模式下应绘制出与手动选区一致的高亮底色
        let prims = ctx.tessellate(output.shapes, 1.0);
        let mut found = false;
        let sel_bg = egui::Visuals::dark().selection.bg_fill;
        for p in prims {
            if let egui::epaint::Primitive::Mesh(mesh) = p.primitive
                && mesh.vertices.iter().any(|v| v.color == sel_bg)
            {
                found = true;
                break;
            }
        }
        assert!(found, "全选模式下应绘制选区高亮底色");
    }

    #[test]
    fn terminal_buffer_tracks_rx_only() {
        let mut app = make_app();
        app.append_rx(b"hello");
        app.append_tx(b"world");
        // 终端显示只包含接收数据（发送数据不混入终端，由自动回显单独处理）
        assert_eq!(app.terminal_buffer, b"hello");
    }

    #[test]
    fn terminal_text_builds_without_timestamps_when_disabled() {
        let mut app = make_app();
        app.config.show_timestamps = false;
        app.append_rx(b"hello\n");
        app.refresh_terminal_text();
        // 未开启时间戳时行头显示绿色 #（# 存于文本，颜色由 marker span 渲染）
        assert_eq!(app.terminal_text, "# hello\n");
        assert!(!app.terminal_text.contains("[RX]"));
        assert!(!app.terminal_text.contains('['));
        assert_eq!(app.terminal_marker_spans.len(), 1);
    }

    #[test]
    fn terminal_text_includes_timestamps_when_enabled() {
        // 「显示时间戳」开启时，终端模式同样显示 [HH:MM:SS.mmm] 提示符前缀，
        // 但不带 [RX]/[TX] 方向标记（仿照 shell）
        let mut app = make_app();
        app.config.show_timestamps = true;
        app.append_rx(b"hello\n");
        app.refresh_terminal_text();
        assert!(
            app.terminal_text.starts_with("[")
                && app.terminal_text.contains("]# hello\n"),
            "终端文本应包含 时间戳+# 行头: {:?}",
            app.terminal_text
        );
        assert!(
            !app.terminal_text.contains("[RX]") && !app.terminal_text.contains("[TX]"),
            "终端文本不应包含方向标记: {:?}",
            app.terminal_text
        );
        // 关闭后重建缓存，标记消失
        app.config.show_timestamps = false;
        app.terminal_cache_invalid = true;
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "# hello\n");
    }

    #[test]
    fn terminal_text_preserves_stream_without_batch_newlines() {
        // 一批数据不得强制换行：分两批到达但未收到换行时，应拼在同一行
        let mut app = make_app();
        app.config.show_timestamps = false;
        app.append_rx(b"hel");
        app.refresh_terminal_text();
        app.append_rx(b"lo\n");
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "# hello\n");

        // 收到换行后的新数据才另起一行并补前缀
        app.append_rx(b"world");
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "# hello\n# world");
    }

    #[test]
    fn terminal_text_prefixes_every_real_line_when_timestamps_enabled() {
        let mut app = make_app();
        app.config.show_timestamps = true;
        // 一批内多个换行：每个真实换行后的行首都补时间戳，不额外产生空行
        app.append_rx(b"a\nb\n\nc");
        app.refresh_terminal_text();
        let text = &app.terminal_text;
        assert!(
            text.starts_with('[') && text.contains("]# a\n"),
            "首行应以 时间戳+# 开头: {text:?}"
        );
        assert!(
            text.contains("\n[") && text.contains("]# b\n\n["),
            "第二行/第四行行首应有时间戳+# 且空行保留: {text:?}"
        );
        assert_eq!(
            app.terminal_marker_spans.len(),
            3,
            "a/b/c 三行内容应各有 1 个行首前缀（空行不加）: {text:?}"
        );
    }

    #[test]
    fn terminal_text_hex_mode() {
        let mut app = make_app();
        app.config.display_mode = DisplayMode::Hex;
        app.append_rx(&[0x01, 0x0A, 0xFF]);
        app.refresh_terminal_text();
        assert!(app.terminal_text.contains("01 0A FF"));
    }

    #[test]
    fn terminal_job_parses_ansi_colors() {
        let font_id = egui::FontId::monospace(14.0);
        let job = terminal_job(&font_id, "\u{1b}[1;31mred\u{1b}[0mplain");
        assert_eq!(job.text, "redplain");
        assert_eq!(job.sections.len(), 2);
        // 加粗 + 红色（31）→ 亮红
        assert_eq!(job.sections[0].format.color, ANSI_BRIGHT_COLORS[1]);
        assert_eq!(job.sections[0].format.background, egui::Color32::TRANSPARENT);
        // 重置后回到默认前景色
        assert_eq!(job.sections[1].format.color, theme::TERMINAL_TEXT);
    }

    #[test]
    fn terminal_job_hides_other_csi_sequences() {
        let font_id = egui::FontId::monospace(14.0);
        // 光标移动等 CSI 序列不显示；颜色自 SGR 之后生效
        let job = terminal_job(&font_id, "a\u{1b}[2Jb\u{1b}[32mc");
        assert_eq!(job.text, "abc");
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].format.color, theme::TERMINAL_TEXT);
        assert_eq!(job.sections[1].format.color, ANSI_COLORS[2]);
    }

    #[test]
    fn terminal_job_crlf_produces_single_linebreak() {
        let font_id = egui::FontId::monospace(14.0);
        // CRLF 必须只产生一次换行，不能渲染出空行
        let job = terminal_job(&font_id, "a\r\nb");
        assert_eq!(job.text, "a\nb");
        // 孤立 CR 也按一次换行处理
        let job = terminal_job(&font_id, "a\rb");
        assert_eq!(job.text, "a\nb");
        // 两段 CRLF（一个空行）仍只产生对应数量的换行
        let job = terminal_job(&font_id, "a\r\n\r\nb");
        assert_eq!(job.text, "a\n\nb");
    }

    #[test]
    fn terminal_area_renders_dark_bg_ansi_text_and_pending_input() {
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.append_rx(b"\x1b[32mOK\x1b[0m\n");
        app.refresh_terminal_text();
        // 回车发送模式下，待发送输入行直接显示在终端里
        app.terminal_input = "AT".to_string();
        app.terminal_cursor = 2;
        let ctx = egui::Context::default();
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(400.0, 200.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| app.receive_area(ui),
            );
        });

        let texts: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|c| {
                if let egui::epaint::Shape::Text(ts) = &c.shape {
                    Some(ts.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            texts.iter().any(|t| t.contains("OK")),
            "终端应显示解码后的 ANSI 文本"
        );
        assert!(
            texts.iter().any(|t| t.contains("AT")),
            "待发送输入行应直接显示在终端内"
        );

        // 深色背景（终端底色）应被绘制
        let prims = ctx.tessellate(output.shapes, 1.0);
        let found = prims.iter().any(|p| {
            if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                m.vertices.iter().any(|v| v.color == theme::TERMINAL_BG)
            } else {
                false
            }
        });
        assert!(found, "终端模式应绘制深色背景");
    }

    #[test]
    fn terminal_paste_event_inserts_or_sends() {
        // 终端右键菜单「粘贴」最终通过 Event::Paste 进入键盘处理：
        // 回车发送开启 → 插入待发送输入行；关闭 → 直接发送。
        let ctx = egui::Context::default();
        for enter_sends in [true, false] {
            let mut app = make_app();
            app.config.terminal_enter_sends = enter_sends;
            let _ = run_ui(
                &ctx,
                egui::RawInput {
                    events: vec![egui::Event::Paste("AB".to_string())],
                    focused: true,
                    ..Default::default()
                },
                |ui| app.handle_terminal_keys(ui),
            );
            if enter_sends {
                assert_eq!(app.terminal_input, "AB", "回车发送模式下粘贴应进入输入行");
            } else {
                assert_eq!(app.terminal_buffer, b"AB", "直接发送模式下粘贴应发送字节");
            }
        }
    }

    #[test]
    fn terminal_prompt_shows_timestamp_and_input_follows() {
        let ctx = egui::Context::default();
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.config.show_timestamps = true;
        app.config.terminal_enter_sends = true;
        app.append_rx(b"OK\n");
        app.refresh_terminal_text();
        let render = |app: &mut SerialApp| {
            run_ui(&ctx, Default::default(), |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(400.0, 200.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| app.receive_area(ui),
                );
            })
        };
        // 首帧：终端渲染后提示符自动生成（时间戳格式）
        render(&mut app);
        assert!(
            app.terminal_prompt.starts_with('[') && app.terminal_prompt.ends_with("# "),
            "应生成 时间戳+# 提示符: {:?}",
            app.terminal_prompt
        );
        // 输入内容应紧跟在提示符之后
        app.terminal_input = "AT".to_string();
        app.terminal_cursor = 2;
        let output = render(&mut app);
        let prompt_and_input = format!("{}{}", app.terminal_prompt, "AT");
        let found = output.shapes.iter().any(|c| {
            if let egui::epaint::Shape::Text(ts) = &c.shape {
                ts.galley.job.text.contains(&prompt_and_input)
            } else {
                false
            }
        });
        assert!(
            found,
            "输入应显示在提示符之后（提示符 {:?}）",
            app.terminal_prompt
        );
        // 回车发送后提示符刷新（新一行）
        app.terminal_send_line("AT");
        assert!(
            !app.terminal_prompt.is_empty(),
            "发送后应生成新提示符"
        );
    }

    #[test]
    fn terminal_timestamp_markers_rendered_green() {
        // 显示时间戳开启时，接收数据的 [HH:MM:SS.mmm] 前缀以绿色渲染，
        // 数据本身保持默认色（关闭回车发送模式以排除实时提示符的干扰）
        let ctx = egui::Context::default();
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.config.show_timestamps = true;
        app.config.terminal_enter_sends = false;
        app.append_rx(b"hello\n");
        app.refresh_terminal_text();
        let output = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(400.0, 200.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| app.receive_area(ui),
            );
        });
        let prims = ctx.tessellate(output.shapes, 1.0);
        let mut green = false;
        let mut white = false;
        for p in prims {
            if let egui::epaint::Primitive::Mesh(mesh) = p.primitive {
                for v in mesh.vertices {
                    if v.color == theme::TERMINAL_PROMPT {
                        green = true;
                    }
                    if v.color == theme::TERMINAL_TEXT {
                        white = true;
                    }
                }
            }
        }
        assert!(green, "时间戳标记应以绿色渲染");
        assert!(white, "数据文本应以默认色渲染");
    }

    #[test]
    fn terminal_shortcuts_select_all_and_copy() {
        let ctx = egui::Context::default();
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.config.terminal_enter_sends = true;
        app.append_rx(b"hello");
        app.refresh_terminal_text();
        let command = egui::Modifiers::COMMAND;
        let key = |k: egui::Key| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: command,
        };
        // Ctrl+A：全选标记
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events: vec![key(egui::Key::A)],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        let select_all = ctx.data(|d| {
            d.get_temp::<bool>(egui::Id::new(RECV_SELECT_ALL_KEY))
                .unwrap_or(false)
        });
        assert!(select_all, "Ctrl+A 应触发全选");
        // Ctrl+C：复制终端文本
        let output = run_ui(
            &ctx,
            egui::RawInput {
                events: vec![key(egui::Key::C)],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        let copied = output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t.contains("hello")));
        assert!(copied, "Ctrl+C 应复制终端文本");
    }

    #[test]
    fn terminal_input_editing_insert_backspace_delete() {
        let mut app = make_app();
        app.terminal_input_insert("AB");
        assert_eq!(app.terminal_input, "AB");
        assert_eq!(app.terminal_cursor, 2);

        // 光标移到中间再插入
        app.terminal_cursor = 1;
        app.terminal_input_insert("X");
        assert_eq!(app.terminal_input, "AXB");
        assert_eq!(app.terminal_cursor, 2);

        // 退格删除光标前字符
        app.terminal_input_backspace();
        assert_eq!(app.terminal_input, "AB");
        assert_eq!(app.terminal_cursor, 1);

        // Delete 删除光标处字符
        app.terminal_input_delete();
        assert_eq!(app.terminal_input, "A");
    }

    #[test]
    fn terminal_enter_sends_accumulates_line_then_sends_with_cr() {
        let mut app = make_app();
        app.config.terminal_auto_echo = true;
        app.config.terminal_enter_sends = true;
        let ctx = egui::Context::default();

        // 打字只进入待发送输入行，不立即发送
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::Text("AT".to_string())],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        assert_eq!(app.terminal_input, "AT");
        assert!(app.terminal_buffer.is_empty());

        // 回车发送整行 + CR（CR 只发送给串口）；自动回显显示命令并本地换行
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        assert!(app.terminal_input.is_empty());
        assert_eq!(
            app.terminal_buffer,
            b"AT\n",
            "回显应为命令 + 本地换行（不含 CR）"
        );
    }

    #[test]
    fn terminal_received_data_after_enter_shows_on_new_line() {
        let mut app = make_app();
        app.config.terminal_auto_echo = true;
        app.config.terminal_enter_sends = true;
        app.config.show_timestamps = false;
        let ctx = egui::Context::default();
        let feed = |ctx: &egui::Context, app: &mut SerialApp, events: Vec<egui::Event>| {
            run_ui(
                ctx,
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| app.handle_terminal_keys(ui),
            );
        };
        feed(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("AT".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        app.refresh_terminal_text();
        // 回车后本地回显已真正换行
        assert_eq!(app.terminal_text, "# AT\n");
        // 对端随后返回的数据必须显示在下一行，而不是拼到 AT 行尾
        app.append_rx(b"OK\r\n");
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "# AT\n# OK\r\n");
    }

    #[test]
    fn terminal_enter_sends_records_command_history() {
        let mut app = make_app();
        app.config.terminal_enter_sends = true;
        let ctx = egui::Context::default();
        let feed = |ctx: &egui::Context, app: &mut SerialApp, events: Vec<egui::Event>| {
            run_ui(
                ctx,
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ui| app.handle_terminal_keys(ui),
            );
        };
        // 输入 "AT" 回车 → 记入历史
        feed(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("AT".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.send_history, vec!["AT"]);
        // 再次发送相同命令：去重，不重复入列
        feed(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("AT".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.send_history, vec!["AT"]);
        // 新命令置顶，旧命令后移
        feed(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("ATE1".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.send_history, vec!["ATE1", "AT"]);
        // 空白命令不记录
        feed(
            &ctx,
            &mut app,
            vec![
                egui::Event::Text("   ".to_string()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(app.send_history, vec!["ATE1", "AT"]);
    }

    #[test]
    fn terminal_arrows_browse_command_history_like_shell() {
        let mut app = make_app();
        app.config.terminal_enter_sends = true;
        app.send_history = vec!["PING".to_string(), "ATE1".to_string(), "AT".to_string()];
        let ctx = egui::Context::default();
        let arrow = |k: egui::Key| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let feed = |ctx: &egui::Context, app: &mut SerialApp, ev: egui::Event| {
            run_ui(
                ctx,
                egui::RawInput {
                    focused: true,
                    events: vec![ev],
                    ..Default::default()
                },
                |ui| app.handle_terminal_keys(ui),
            );
        };

        // 输入半截草稿后按 ↑：保存草稿并逐条向旧命令翻
        feed(&ctx, &mut app, egui::Event::Text("P".to_string()));
        feed(&ctx, &mut app, arrow(egui::Key::ArrowUp));
        assert_eq!(app.terminal_input, "PING");
        assert_eq!(app.terminal_cursor, 4);
        assert_eq!(app.terminal_history_draft, "P");
        assert_eq!(app.terminal_history_index, Some(0));

        feed(&ctx, &mut app, arrow(egui::Key::ArrowUp));
        assert_eq!(app.terminal_input, "ATE1");
        assert_eq!(app.terminal_history_index, Some(1));

        feed(&ctx, &mut app, arrow(egui::Key::ArrowUp));
        assert_eq!(app.terminal_input, "AT");
        assert_eq!(app.terminal_history_index, Some(2));

        // 已是最旧一条：继续 ↑ 保持不变
        feed(&ctx, &mut app, arrow(egui::Key::ArrowUp));
        assert_eq!(app.terminal_input, "AT");
        assert_eq!(app.terminal_history_index, Some(2));

        // ↓ 逐条向新翻，最后回到草稿
        feed(&ctx, &mut app, arrow(egui::Key::ArrowDown));
        assert_eq!(app.terminal_input, "ATE1");
        feed(&ctx, &mut app, arrow(egui::Key::ArrowDown));
        assert_eq!(app.terminal_input, "PING");
        feed(&ctx, &mut app, arrow(egui::Key::ArrowDown));
        assert_eq!(app.terminal_input, "P");
        assert_eq!(app.terminal_history_index, None);

        // 历史为空时 ↑ 无副作用
        let mut empty = make_app();
        empty.config.terminal_enter_sends = true;
        empty.terminal_input = "hi".to_string();
        feed(&ctx, &mut empty, arrow(egui::Key::ArrowUp));
        assert_eq!(empty.terminal_input, "hi");
        assert_eq!(empty.terminal_history_index, None);

        // 浏览后直接回车发送：发送后退出浏览状态
        feed(&ctx, &mut app, arrow(egui::Key::ArrowUp));
        feed(&ctx, &mut app, arrow(egui::Key::Enter));
        assert_eq!(app.terminal_input, "");
        assert_eq!(app.terminal_history_index, None);
        assert_eq!(app.terminal_history_draft, "");
        assert_eq!(app.send_history.first().map(String::as_str), Some("PING"));
    }

    #[test]
    fn terminal_immediate_keys_send_and_echo() {
        let mut app = make_app();
        app.config.terminal_auto_echo = true;
        app.config.terminal_enter_sends = false;
        let ctx = egui::Context::default();
        let _ = run_ui(
            &ctx,
            egui::RawInput {
                events: vec![
                    egui::Event::Text("A".to_string()),
                    egui::Event::Text("B".to_string()),
                ],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        assert!(app.terminal_input.is_empty());
        assert_eq!(app.terminal_buffer, b"AB");
    }

    #[test]
    fn terminal_immediate_mode_keeps_hash_prompt_and_enter_breaks_line() {
        let ctx = egui::Context::default();
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.config.terminal_auto_echo = true;
        app.config.terminal_enter_sends = false;
        app.config.show_timestamps = false;
        // 空终端渲染一帧：即时发送模式下也应生成行头 # 提示符
        let _ = run_ui(&ctx, Default::default(), |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(400.0, 200.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| app.receive_area(ui),
            );
        });
        assert_eq!(app.terminal_prompt, "# ", "即时模式空行也应显示 # 提示符");

        let key = |k: egui::Key| egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let feed = |ctx: &egui::Context, app: &mut SerialApp, ev: egui::Event| {
            run_ui(
                ctx,
                egui::RawInput {
                    focused: true,
                    events: vec![ev],
                    ..Default::default()
                },
                |ui| app.handle_terminal_keys(ui),
            );
        };
        for c in ['A', 'T'] {
            feed(&ctx, &mut app, egui::Event::Text(c.to_string()));
        }
        feed(&ctx, &mut app, key(egui::Key::Enter));
        // 回显内容为 "AT" + 本地换行（CR 只发送给串口，不回显）
        assert_eq!(app.terminal_buffer, b"AT\n");
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "# AT\n");
    }

    #[test]
    fn receive_settings_row_controls_in_order() {
        let ctx = egui::Context::default();
        let mut app = make_app();
        app.config.language = Language::English;
        let s = Language::English.strings();
        let w = 1100.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w, 120.0))),
            ..Default::default()
        };
        let output = run_ui(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.set_width(w - 16.0);
                app.receive_settings_row(ui);
            });
        });
        // 顺序：显示 < 编码 < 终端模式 < 回车发送 < 本地回显 < 自动滚动 <
        // 显示时间戳 < 暂停接收 < 记录日志 < 打开日志路径 < 清空显示 < 清空统计
        // （换行时按 (y, x) 比较）
        let labels = [
            s.display,
            s.encoding,
            s.terminal_mode,
            s.enter_sends,
            s.auto_echo,
            s.auto_scroll,
            s.show_timestamps,
            s.pause_receive,
            s.record_log,
            s.open_log_path,
            s.clear_display,
            s.clear_stats,
        ];
        let pos_of = |label: &str| -> egui::Pos2 {
            output
                .shapes
                .iter()
                .filter_map(|c| {
                    if let egui::epaint::Shape::Text(ts) = &c.shape {
                        (ts.galley.job.text == label).then_some(ts.pos)
                    } else {
                        None
                    }
                })
                .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
                .unwrap_or_else(|| panic!("缺少控件文本: {label}"))
        };
        let mut prev = pos_of(labels[0]);
        for label in &labels[1..] {
            let p = pos_of(label);
            assert!(
                p.y > prev.y + 0.5 || (p.y - prev.y).abs() < 0.5 && p.x > prev.x,
                "控件顺序异常: {label} 在 {p:?}，前一控件在 {prev:?}"
            );
            prev = p;
        }
    }
}
