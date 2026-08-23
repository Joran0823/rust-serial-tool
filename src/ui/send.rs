//! 发送区与队列区。
//! 布局对应 docs/UI-Desing.svg（2026-08-01 版）的发送区与队列区。

use crate::app::SerialApp;
use crate::codec;
use crate::config::{LineEnding, QueueItem, SendMode, SendQueue};
use crate::i18n::Language;
use crate::queue_file;
use crate::serial::{Command, QueueSendItem};
use crate::ui::theme;
use crate::ui::widgets;
use eframe::egui;

impl SerialApp {
    /// 布局3-子1：发送文本框（可读写，填充父控件大小）。
    pub fn send_input_box(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        let resp = ui.add_sized(
            [ui.available_width(), ui.available_height()],
            egui::TextEdit::multiline(&mut self.send_input)
                .font(egui::TextStyle::Monospace)
                .horizontal_align(egui::Align::Min)
                .vertical_align(egui::Align::Min)
                .hint_text(s.send_input_hint)
                .desired_width(f32::INFINITY)
                .background_color(egui::Color32::WHITE),
        );
        widgets::text_edit_context_menu(ui, &resp, true, &self.send_input, s);
    }

    /// 布局3-子2：发送按钮行（发送/清空发送/添加到队列/模式/行尾/历史/定时发送/间隔，全部垂直居中）。
    pub fn send_buttons_row(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
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
            ui.label(egui::RichText::new(s.mode).color(theme::TEXT_SOFT));
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
            ui.label(egui::RichText::new(s.line_ending).color(theme::TEXT_SOFT));
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
            ui.label(egui::RichText::new(s.history).color(theme::TEXT_SOFT));
            let first = self.send_history.first().cloned().unwrap_or_default();
            // 除历史下拉框外控件宽度固定，剩余宽度自动分配给历史下拉框（右侧预留定时发送组）
            let hist_w = (ui.available_width() - 230.0).max(90.0);
            widgets::combo(
                ui,
                hist_w,
                26.0,
                true,
                if first.is_empty() { "—".to_string() } else { first.clone() },
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
                ui.label(egui::RichText::new("ms").color(theme::TEXT_SOFT));
                let resp = ui.add_sized(
                    [62.0, 22.0],
                    egui::DragValue::new(&mut self.config.periodic_interval_ms)
                        .range(10..=3_600_000),
                );
                resp.on_hover_text(s.interval_tip);
                ui.label(egui::RichText::new(s.interval).color(theme::TEXT_SOFT));
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

    /// 队列区：头部（队列选择/删除/复制/新建/导出/导入/清空/添加/队列发送）+ 条目列表。
    pub fn queue_panel(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        // ---- 队列级操作行：固定 33px 高度容器，避免在剩余高度内垂直居中产生偏移 ----
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 33.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(s.queue);
            let names: Vec<String> = self.config.queues.iter().map(|q| q.name.clone()).collect();
            let cur = names
                .get(self.config.selected_queue)
                .cloned()
                .unwrap_or_default();
            widgets::combo(
                ui,
                90.0,
                33.0,
                true,
                cur,
                s.queue_tip,
                |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.config.selected_queue, i, n);
                    }
                },
            );

            if ui
                .add_sized([44.0, 33.0], theme::secondary_widget(s.delete))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.delete_queue_tip)
                .clicked()
                && self.config.queues.len() > 1
            {
                self.config.queues.remove(self.config.selected_queue);
                self.config.selected_queue = self
                    .config
                    .selected_queue
                    .min(self.config.queues.len() - 1);
            }
            if ui
                .add_sized([52.0, 33.0], theme::secondary_widget(s.copy))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.copy_queue_tip)
                .clicked()
                && let Some(q) = self.config.queues.get(self.config.selected_queue).cloned()
            {
                let mut copy = q.clone();
                copy.name = s.fill(s.queue_copy_suffix, &[("name", q.name.clone())]);
                self.config.queues.push(copy);
                self.config.selected_queue = self.config.queues.len() - 1;
            }
            if ui
                .add_sized([52.0, 33.0], theme::secondary_widget(s.new))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.new_queue_tip)
                .clicked()
            {
                let name = s.fill(
                    s.queue_new_name,
                    &[("n", (self.config.queues.len() + 1).to_string())],
                );
                self.config.queues.push(SendQueue::new(name));
                self.config.selected_queue = self.config.queues.len() - 1;
            }
            if ui
                .add_sized([52.0, 33.0], theme::secondary_widget(s.export))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.export_queue_tip)
                .clicked()
            {
                self.save_queue_to_file();
            }
            if ui
                .add_sized([52.0, 33.0], theme::secondary_widget(s.import))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.import_queue_tip)
                .clicked()
            {
                self.import_queue_items();
            }
            if ui
                .add_sized([122.0, 33.0], theme::secondary_widget(s.clear_queue_items))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.clear_queue_items_tip)
                .clicked()
                && let Some(q) = self.config.queues.get_mut(self.config.selected_queue)
            {
                q.items.clear();
            }
            if ui
                .add_sized([124.0, 33.0], theme::secondary_widget(s.add_queue_item))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.add_queue_item_tip)
                .clicked()
                && let Some(q) = self.config.queues.get_mut(self.config.selected_queue)
                && q.items.len() < self.config.max_queue_items
            {
                q.items.push(QueueItem::default());
            }

            if self.queue_sending {
                if ui
                    .add_sized([96.0, 33.0], theme::primary_widget(s.stop_send))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.stop_send_tip)
                    .clicked()
                {
                    self.session.send(Command::StopQueue);
                }
                if let Some((c, t)) = self.queue_progress {
                    ui.label(
                        egui::RichText::new(format!("{c}/{t}"))
                            .color(theme::OK_GREEN),
                    );
                }
            } else if ui
                .add_sized([96.0, 33.0], theme::primary_widget(s.queue_send))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(s.queue_send_tip)
                .clicked()
            {
                self.start_queue_send();
            }
        });
            },
        );

        // ---- 条目列表 ----
        let mut send_one: Option<Vec<u8>> = None;
        let mut status_msg: Option<(String, bool)> = None;
        let mut moved_up: Option<usize> = None;
        let mut moved_down: Option<usize> = None;
        let mut removed: Option<usize> = None;
        let mut copied: Option<usize> = None;
        let max_items = self.config.max_queue_items;
        let lang = self.config.language;
        {
            let sel = self.config.selected_queue;
            let le = self.config.line_ending;
            let queue = &mut self.config.queues[sel];

            let list_h = ui.available_height().max(40.0);
            egui::ScrollArea::vertical()
                .id_salt("queue_items")
                .auto_shrink([false, false])
                .max_height(list_h)
                .show(ui, |ui| {
                    if queue.items.is_empty() {
                        ui.label(
                            egui::RichText::new(s.no_items_hint)
                                .color(theme::TEXT_SOFT),
                        );
                    }
                    let spacing = ui.spacing().item_spacing.x;
                    let sel_w = 22.0;
                    let mode_w = 70.0;
                    let send_w = 72.0;
                    let delay_w = 86.0;
                    let icon_w = 18.0;
                    let fixed = sel_w
                        + mode_w
                        + send_w
                        + delay_w
                        + icon_w * 4.0
                        + spacing * 8.0;
                    let content_w = (ui.available_width() - fixed).max(80.0);

                    for i in 0..queue.items.len() {
                        let item = &mut queue.items[i];
                        // 每个条目一个内容横向布局：高度固定、宽度自适应、内容垂直居中
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 30.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                            ui.add_sized(
                                [sel_w, 24.0],
                                egui::Checkbox::new(&mut item.selected, ""),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(s.item_checked_tip);
                            widgets::combo(
                                ui,
                                mode_w,
                                24.0,
                                true,
                                item.mode.label(lang),
                                s.item_mode_tip,
                                |ui| {
                                    ui.selectable_value(
                                        &mut item.mode,
                                        SendMode::Text,
                                        s.text_mode,
                                    );
                                    ui.selectable_value(&mut item.mode, SendMode::Hex, "HEX");
                                },
                            );
                            let content_edit = ui.add(
                                egui::TextEdit::singleline(&mut item.content)
                                    .desired_width(content_w)
                                    .hint_text(s.content_hint),
                            );
                            widgets::text_edit_context_menu(
                                ui,
                                &content_edit,
                                true,
                                &item.content,
                                s,
                            );
                            if ui
                                .add_sized([send_w, 27.0], theme::outline_widget(s.send))
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(s.send_item_tip)
                                .clicked()
                            {
                                match item_bytes(item, le, lang) {
                                    Ok(b) => send_one = Some(b),
                                    Err(e) => status_msg = Some((e, true)),
                                }
                            }
                            ui.add_sized(
                                [delay_w, 22.0],
                                egui::DragValue::new(&mut item.delay_ms)
                                    .range(0..=600_000)
                                    .suffix(" ms"),
                            );
                            if up_button(ui, egui::vec2(icon_w, icon_w))
                                .on_hover_text(s.move_up)
                                .clicked()
                            {
                                moved_up = Some(i);
                            }
                            if down_button(ui, egui::vec2(icon_w, icon_w))
                                .on_hover_text(s.move_down)
                                .clicked()
                            {
                                moved_down = Some(i);
                            }
                            if copy_button(ui, egui::vec2(icon_w, icon_w))
                                .on_hover_text(s.copy_item_tip)
                                .clicked()
                            {
                                copied = Some(i);
                            }
                            if trash_button(ui, egui::vec2(icon_w, icon_w))
                                .on_hover_text(s.delete_item_tip)
                                .clicked()
                            {
                                removed = Some(i);
                            }
                            },
                        );
                    }
                });

            if let Some(i) = moved_up
                && i > 0
            {
                queue.items.swap(i, i - 1);
            }
            if let Some(i) = moved_down
                && i + 1 < queue.items.len()
            {
                queue.items.swap(i, i + 1);
            }
            if let Some(i) = copied
                && queue.items.len() < max_items
                && let Some(c) = queue.items.get(i).cloned()
            {
                queue.items.insert(i + 1, c);
            }
            if let Some(i) = removed {
                queue.items.remove(i);
            }
        }

        if let Some(bytes) = send_one {
            let n = bytes.len();
            self.session.send(Command::Write(bytes));
            self.set_status(
                self.t()
                    .fill(self.t().sent_queue_item_fmt, &[("n", n.to_string())]),
                false,
            );
        }
        if let Some((m, is_err)) = status_msg {
            self.set_status(m, is_err);
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

    pub(crate) fn start_file_send(&mut self) {
        let Some(path) = self.pending_file.clone() else {
            self.set_status(self.t().choose_file_first.to_string(), true);
            return;
        };
        let mode = self.config.file_mode;
        let line_ending = self.config.line_ending.bytes().to_vec();
        let interval = self.config.file_line_interval_ms;
        self.session.send(Command::StartFile {
            path,
            mode,
            line_ending,
            line_interval_ms: interval,
        });
    }

    /// 队列发送：忽略未勾选与空条目；全部不可发送时弹窗警告。
    fn start_queue_send(&mut self) {
        let Some(queue) = self.config.queues.get(self.config.selected_queue).cloned() else {
            return;
        };
        if queue.items.is_empty() {
            self.alert = Some(self.t().queue_empty_alert.to_string());
            return;
        }
        let le = self.config.line_ending;
        let lang = self.config.language;
        let mut items = Vec::with_capacity(queue.items.len());
        let mut skipped = 0usize;
        for (i, item) in queue.items.iter().enumerate() {
            if !item.selected {
                skipped += 1;
                continue;
            }
            if item.content.trim().is_empty() {
                skipped += 1;
                continue;
            }
            match item_bytes(item, le, lang) {
                Ok(bytes) if bytes.is_empty() => skipped += 1,
                Ok(bytes) => items.push(QueueSendItem {
                    bytes,
                    delay_ms: item.delay_ms,
                }),
                Err(e) => {
                    self.set_status(
                        self.t().fill(
                            self.t().invalid_item_fmt,
                            &[
                                ("index", (i + 1).to_string()),
                                ("e", e),
                            ],
                        ),
                        true,
                    );
                    return;
                }
            }
        }
        if items.is_empty() {
            self.alert = Some(self.t().nothing_to_send_alert.to_string());
            return;
        }
        if skipped > 0 {
            self.set_status(
                self.t()
                    .fill(self.t().skipped_items_fmt, &[("skipped", skipped.to_string())]),
                false,
            );
        }
        self.session.send(Command::StartQueue {
            name: queue.name,
            items,
        });
    }

    fn add_current_to_queue(&mut self) {
        let content = self.send_input.clone();
        if content.trim().is_empty() {
            self.set_status(self.t().empty_send.to_string(), true);
            return;
        }
        let mut ok = false;
        {
            let sel = self.config.selected_queue;
            let max = self.config.max_queue_items;
            if let Some(q) = self.config.queues.get_mut(sel)
                && q.items.len() < max
            {
                q.items.push(QueueItem {
                    mode: self.config.send_mode,
                    content,
                    delay_ms: 100,
                    selected: true,
                });
                ok = true;
            }
        }
        if ok {
            self.set_status(self.t().added_to_queue.to_string(), false);
        } else {
            self.set_status(self.t().queue_full.to_string(), true);
        }
    }

    /// 导出当前队列为 TOML 文件。
    fn save_queue_to_file(&mut self) {
        let Some(queue) = self.config.queues.get(self.config.selected_queue).cloned() else {
            return;
        };
        let base = if queue.name.trim().is_empty() {
            self.t().queue_base_name.to_string()
        } else {
            queue.name.trim().to_string()
        };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(self.t().queue_file_filter, &["toml"])
            .set_file_name(format!("{base}.toml"))
            .save_file()
        {
            match std::fs::write(&path, queue_file::export_queue_toml(&queue)) {
                Ok(()) => {
                    self.set_status(
                        self.t().fill(
                            self.t().queue_exported_fmt,
                            &[("path", path.display().to_string())],
                        ),
                        false,
                    )
                }
                Err(e) => {
                    self.set_status(
                        self.t()
                            .fill(self.t().export_failed_fmt, &[("e", e.to_string())]),
                        true,
                    );
                }
            }
        }
    }

    /// 从 TOML（完整队列）或 TXT（每行一条）导入条目到当前队列。
    fn import_queue_items(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(self.t().queue_file_filter, &["toml"])
            .add_filter(self.t().text_file_filter, &["txt"])
            .add_filter(self.t().all_files, &["*"])
            .pick_file()
        else {
            return;
        };
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                self.set_status(
                    self.t()
                        .fill(self.t().read_file_failed_fmt, &[("e", e.to_string())]),
                    true,
                );
                return;
            }
        };
        let is_txt = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("txt"))
            .unwrap_or(false);
        let imported: Vec<QueueItem> = if is_txt {
            queue_file::import_queue_txt(&content)
        } else {
            match queue_file::import_queue_toml(&content, self.config.language) {
                Ok(q) => q.items,
                Err(e) => {
                    self.set_status(
                        self.t()
                            .fill(self.t().import_failed_fmt, &[("e", e.to_string())]),
                        true,
                    );
                    return;
                }
            }
        };
        if imported.is_empty() {
            self.set_status(self.t().no_importable_items.to_string(), true);
            return;
        }

        let max = self.config.max_queue_items;
        let mut added = 0usize;
        let total = imported.len();
        {
            let sel = self.config.selected_queue;
            if let Some(q) = self.config.queues.get_mut(sel) {
                for item in imported {
                    if q.items.len() >= max {
                        break;
                    }
                    q.items.push(item);
                    added += 1;
                }
            }
        }
        if added == 0 {
            self.set_status(
                self.t()
                    .fill(self.t().queue_limit_reached_fmt, &[("max", max.to_string())]),
                true,
            );
        } else if added < total {
            self.set_status(
                self.t().fill(
                    self.t().imported_some_fmt,
                    &[
                        ("added", added.to_string()),
                        ("skipped", (total - added).to_string()),
                    ],
                ),
                false,
            );
        } else {
            self.set_status(
                self.t()
                    .fill(self.t().imported_fmt, &[("added", added.to_string())]),
                false,
            );
        }
    }
}

fn item_bytes(item: &QueueItem, le: LineEnding, lang: Language) -> Result<Vec<u8>, String> {
    match item.mode {
        SendMode::Text => {
            let mut b = item.content.as_bytes().to_vec();
            b.extend_from_slice(le.bytes());
            Ok(b)
        }
        SendMode::Hex => codec::hex::parse_hex(&item.content, lang),
    }
}

/// 通用图标按钮：分配点击区域并调用绘制函数。
fn icon_button(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    draw: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if resp.hovered() {
            theme::BLUE
        } else {
            theme::TEXT_SOFT
        };
        draw(ui.painter(), rect, color);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// 上移（↑）
fn up_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    icon_button(ui, size, |p, rect, color| {
        let c = rect.center();
        let stroke = egui::Stroke::new(1.6, color);
        p.line_segment(
            [egui::pos2(c.x - 4.0, c.y + 3.0), egui::pos2(c.x, c.y - 4.0)],
            stroke,
        );
        p.line_segment(
            [egui::pos2(c.x, c.y - 4.0), egui::pos2(c.x + 4.0, c.y + 3.0)],
            stroke,
        );
    })
}

/// 下移（↓）
fn down_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    icon_button(ui, size, |p, rect, color| {
        let c = rect.center();
        let stroke = egui::Stroke::new(1.6, color);
        p.line_segment(
            [egui::pos2(c.x - 4.0, c.y - 3.0), egui::pos2(c.x, c.y + 4.0)],
            stroke,
        );
        p.line_segment(
            [egui::pos2(c.x, c.y + 4.0), egui::pos2(c.x + 4.0, c.y - 3.0)],
            stroke,
        );
    })
}

/// 复制（两个重叠方框）
fn copy_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    icon_button(ui, size, |p, rect, color| {
        let c = rect.center();
        let stroke = egui::Stroke::new(1.2, color);
        let back = egui::Rect::from_center_size(
            egui::pos2(c.x - 2.0, c.y - 2.0),
            egui::vec2(7.0, 8.0),
        );
        let front = egui::Rect::from_center_size(
            egui::pos2(c.x + 2.0, c.y + 2.0),
            egui::vec2(7.0, 8.0),
        );
        p.rect_stroke(back, 1.0, stroke, egui::StrokeKind::Inside);
        p.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
    })
}

/// 删除（垃圾桶）
fn trash_button(ui: &mut egui::Ui, size: egui::Vec2) -> egui::Response {
    icon_button(ui, size, |p, rect, color| {
        let stroke = egui::Stroke::new(1.4, color);
        let c = rect.center();
        let top = rect.top() + 1.0;
        let lid_y = top + 4.0;
        p.line_segment(
            [
                egui::pos2(c.x - 5.0, lid_y),
                egui::pos2(c.x + 5.0, lid_y),
            ],
            stroke,
        );
        let handle = egui::Rect::from_min_max(
            egui::pos2(c.x - 1.8, top),
            egui::pos2(c.x + 1.8, top + 3.0),
        );
        p.rect_stroke(handle, 0.5, stroke, egui::StrokeKind::Inside);
        let body = egui::Rect::from_min_max(
            egui::pos2(c.x - 4.0, lid_y + 1.5),
            egui::pos2(c.x + 4.0, rect.bottom() - 1.0),
        );
        p.rect_stroke(body, 1.0, stroke, egui::StrokeKind::Inside);
        p.line_segment(
            [
                egui::pos2(c.x, body.top() + 1.0),
                egui::pos2(c.x, body.bottom() - 1.0),
            ],
            stroke,
        );
    })
}
