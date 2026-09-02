//! 发送原始数据面板：发送文本框、按钮行（发送/清空/模式/行尾/历史/定时发送）。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版）的发送区。

use crate::app::SerialApp;
use crate::codec;
use crate::config::{LineEnding, SendMode};
use crate::serial::Command;
use crate::ui::theme;
use crate::ui::widgets;
use eframe::egui;

impl SerialApp {
    /// 发送文本框（可读写）：内部滚动条，文本超出高度时在框内滚动，
    /// 不再依赖 dock 的整体滚动条。
    pub fn send_input_box(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        egui::Frame::new()
            .fill(theme::input_bg())
            .corner_radius(4)
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("send_input_scroll")
                    .auto_shrink([false, false])
                    // ScrollArea 默认 min_scrolled_size=64px，会把输入框撑高；
                    // 高度完全由面板布局决定，文本内部滚动。
                    .min_scrolled_height(0.0)
                    .show(ui, |ui| {
                        let resp = ui.add(
                            egui::TextEdit::multiline(&mut self.send_input)
                                .font(egui::TextStyle::Monospace)
                                .horizontal_align(egui::Align::Min)
                                .vertical_align(egui::Align::Min)
                                .hint_text(s.send_input_hint)
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::NONE)
                                .background_color(egui::Color32::TRANSPARENT),
                        );
                        widgets::text_edit_context_menu(ui, &resp, true, &self.send_input, s);
                    });
            });
    }

    /// 发送按钮行（发送/清空发送/添加到队列/模式/行尾/历史/定时发送/间隔，全部垂直居中）。
    pub fn send_buttons_row(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        // 右侧"定时发送"组实际宽度（英文标签更长），据此给 History 下拉框留足空间，
        // 避免右侧组从右往左溢出、盖住 History 下拉框。
        let periodic_w = periodic_group_width(ui, s.interval);
        let row_spacing = ui.spacing().item_spacing.x;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if ui
                .add_sized([102.0, 32.0], theme::primary_widget(s.send))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.send_tip)
                .clicked()
            {
                self.send_current_input();
            }
            if ui
                .add_sized([102.0, 32.0], theme::secondary_widget(s.clear_send))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.clear_send_tip)
                .clicked()
            {
                self.send_input.clear();
            }
            if ui
                .add_sized([116.0, 32.0], theme::secondary_widget(s.add_to_queue))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.add_to_queue_tip)
                .clicked()
            {
                self.add_current_to_queue();
            }
            // 模式/行尾/发送历史（设计稿未绘制，但属必需功能）
            ui.label(egui::RichText::new(s.mode).color(theme::text_soft()));
            widgets::combo(
                ui,
                64.0,
                26.0,
                true,
                self.config.send_mode.label(self.config.language),
                s.mode_tip,
                |ui| {
                    ui.selectable_value(&mut self.config.send_mode, SendMode::Text, s.text_mode);
                    ui.selectable_value(&mut self.config.send_mode, SendMode::Hex, "HEX");
                },
            );
            ui.label(egui::RichText::new(s.line_ending).color(theme::text_soft()));
            widgets::combo(
                ui,
                90.0,
                26.0,
                true,
                self.config.line_ending.label(self.config.language),
                s.line_ending_tip,
                |ui| {
                    ui.selectable_value(&mut self.config.line_ending, LineEnding::None, s.none);
                    ui.selectable_value(&mut self.config.line_ending, LineEnding::CR, "CR");
                    ui.selectable_value(&mut self.config.line_ending, LineEnding::LF, "LF");
                    ui.selectable_value(&mut self.config.line_ending, LineEnding::CRLF, "CRLF");
                },
            );
            ui.label(egui::RichText::new(s.history).color(theme::text_soft()));
            let first = self.send_history.first().cloned().unwrap_or_default();
            // 剩余宽度减去右侧定时发送组的实际宽度后，自动分配给历史下拉框
            let hist_w = (ui.available_width() - periodic_w - row_spacing - 4.0).max(90.0);
            widgets::combo(
                ui,
                hist_w,
                26.0,
                true,
                if first.is_empty() { "\u{2014}".to_string() } else { first.clone() },
                s.history_tip,
                |ui| {
                    for h in self.send_history.clone() {
                        if ui.button(&h).clicked() {
                            self.send_input = h;
                        }
                    }
                },
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 收紧组内交互控件的最小高度：egui 按 interact_size.y(30) + 按钮内边距
                // 计算复选框/数值框的自然高度，会让它们溢出 22px 的分配区域并向下偏移，
                // 与两侧标签错开。这里把最小高度压到 22px，与 add_sized 分配一致。
                ui.spacing_mut().interact_size.y = 22.0;
                ui.spacing_mut().button_padding.y = 1.0;
                ui.label(egui::RichText::new("ms").color(theme::text_soft()));
                let resp = ui.add_sized(
                    [62.0, 22.0],
                    egui::DragValue::new(&mut self.config.periodic_interval_ms)
                        .range(10..=3_600_000),
                );
                resp.on_hover_text(s.interval_tip);
                ui.label(egui::RichText::new(s.interval).color(theme::text_soft()));
                let mut on = self.periodic_enabled;
                if ui
                    .add_sized([80.0, 22.0], egui::Checkbox::new(&mut on, s.periodic))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.periodic_tip)
                    .changed()
                {
                    self.toggle_periodic(on);
                }
            });
        });
        if self.periodic_enabled {
            ui.label(
                egui::RichText::new(s.periodic_running)
                    .small()
                    .color(theme::OK_GREEN),
            );
        }
    }

    fn toggle_periodic(&mut self, on: bool) {
        if on {
            match self.current_input_bytes() {
                Ok(bytes) if !bytes.is_empty() => {
                    self.session.send(Command::StartPeriodic {
                        bytes,
                        interval_ms: self.config.periodic_interval_ms,
                    });
                }
                Ok(_) => self.set_status(self.t().empty_periodic.to_string(), true),
                Err(e) => self.set_status(e, true),
            }
        } else {
            self.session.send(Command::StopPeriodic);
        }
    }

    fn current_input_bytes(&self) -> Result<Vec<u8>, String> {
        match self.config.send_mode {
            SendMode::Text => {
                let mut b = self.send_input.as_bytes().to_vec();
                b.extend_from_slice(self.config.line_ending.bytes());
                Ok(b)
            }
            SendMode::Hex => codec::hex::parse_hex(&self.send_input, self.config.language),
        }
    }

    fn send_current_input(&mut self) {
        match self.current_input_bytes() {
            Ok(bytes) if bytes.is_empty() => self.set_status(self.t().empty_send.to_string(), true),
            Ok(bytes) => {
                let n = bytes.len();
                self.session.send(Command::Write(bytes));
                let content = self.send_input.trim().to_string();
                if !content.is_empty() {
                    self.send_history.retain(|h| h != &content);
                    self.send_history.insert(0, content);
                    self.send_history.truncate(20);
                }
                self.set_status(
                    self.t()
                        .fill(self.t().sent_bytes_fmt, &[("n", n.to_string())]),
                    false,
                );
            }
            Err(e) => self.set_status(e, true),
        }
    }
}

/// 右侧"定时发送"组的总宽度：Periodic 复选框(80) + 间隔标签 + 数值框(62) + "ms"标签 + 3 段控件间距。
/// 标签宽度按当前语言的字体实际测量，避免英文下预留不足导致控件重叠。
pub(crate) fn periodic_group_width(ui: &mut egui::Ui, interval_label: &str) -> f32 {
    let spacing = ui.spacing().item_spacing.x;
    let font_id = ui.style().text_styles[&egui::TextStyle::Body].clone();
    let measure = |ui: &mut egui::Ui, text: &str| -> f32 {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::WHITE)
        })
        .size()
        .x
    };
    80.0 + measure(ui, interval_label) + 62.0 + measure(ui, "ms") + spacing * 3.0
}
