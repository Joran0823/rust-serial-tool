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
use crate::util;
use eframe::egui;
use std::io::Write;

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
                            let label = egui::Label::new(
                                egui::RichText::new(&self.display_cache)
                                    .monospace()
                                    .color(if select_all {
                                        egui::Color32::WHITE
                                    } else {
                                        theme::text()
                                    }),
                            )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .halign(egui::Align::Min);
                            // 全选高亮：蓝底 + 白字，模拟选区效果
                            let resp = if select_all {
                                egui::Frame::new()
                                    .fill(theme::BLUE_CHECK)
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

    /// 终端模式接收区：深色背景、无时间戳、支持 ANSI 颜色；
    /// 接收显示区本身就是输入区（类似 PuTTY/minicom），点击后直接键盘输入。
    pub(crate) fn terminal_area(&mut self, ui: &mut egui::Ui) {
        let s = self.t();
        self.refresh_terminal_text();

        let avail_h = ui.available_height().max(40.0);
        // 整个终端区域是一个可点击获得焦点的控件，键盘输入直接作用于该区域
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), avail_h),
            egui::Sense::click_and_drag(),
        );
        if resp.clicked() {
            resp.request_focus();
        }
        let focused = resp.has_focus();
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
                                ui.add(egui::Label::new(self.terminal_job_with_input(ui, blink_on)));
                            }
                        });
                });
        });

        // 仅当终端区域拥有键盘焦点时处理输入事件
        if focused {
            self.handle_terminal_keys(ui);
        }
    }

    /// 终端显示内容：ANSI 文本 + 待发送输入行 + 闪烁块状光标。
    fn terminal_job_with_input(&self, ui: &egui::Ui, blink_on: bool) -> egui::text::LayoutJob {
        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
        let mut job = terminal_job(&font_id, &self.terminal_text);
        let chars: Vec<char> = self.terminal_input.chars().collect();
        let cursor = self.terminal_cursor.min(chars.len());
        let before: String = chars[..cursor].iter().collect();
        let after: String = chars[cursor..].iter().collect();
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
                        self.terminal_send_line(&line);
                    } else {
                        self.terminal_send_bytes(b"\r");
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
                }
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    pressed: true,
                    ..
                } if enter_sends => {
                    let n = self.terminal_input.chars().count();
                    self.terminal_cursor = (self.terminal_cursor + 1).min(n);
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

    /// 发送终端输入的整行内容（行尾附 CR），并按「自动回显」决定是否本地显示。
    fn terminal_send_line(&mut self, line: &str) {
        let mut bytes = codec::text::encode(line, self.config.text_encoding);
        bytes.push(b'\r');
        self.terminal_send_bytes(&bytes);
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

    /// 追加字节到终端显示缓冲（仅接收数据与本地回显使用）。
    pub(crate) fn append_terminal_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
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
            self.terminal_decoder = None;
            self.terminal_decoder_enc = None;
            self.terminal_decoded_len = 0;
        }
        if self.terminal_decoded_len >= self.terminal_buffer.len() {
            return;
        }
        let bytes = &self.terminal_buffer[self.terminal_decoded_len..];
        match self.config.display_mode {
            DisplayMode::Text => {
                let enc = self.config.text_encoding;
                if enc == TextEncoding::Ascii {
                    self.terminal_text.push_str(&codec::text::decode(bytes, enc));
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
                        self.terminal_text.push_str(&stream_decode(dec, bytes, false));
                    }
                }
            }
            DisplayMode::Hex => {
                self.terminal_text.push_str(&codec::hex::format_hex(bytes));
                self.terminal_text.push('\n');
            }
        }
        self.terminal_decoded_len = self.terminal_buffer.len();
        if self.terminal_text.len() > TERMINAL_TEXT_CAP {
            let cut =
                self.terminal_text
                    .floor_char_boundary(self.terminal_text.len() - TERMINAL_TEXT_CAP);
            self.terminal_text.drain(..cut);
        }
    }

    /// 清空终端显示缓冲（进入终端模式或点击「清空显示」时调用）。
    pub(crate) fn clear_terminal_display(&mut self) {
        self.terminal_buffer.clear();
        self.terminal_text.clear();
        self.terminal_decoder = None;
        self.terminal_decoder_enc = None;
        self.terminal_cache_invalid = false;
        self.terminal_decoded_len = 0;
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
fn terminal_job(font_id: &egui::FontId, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut fg = theme::TERMINAL_TEXT;
    let mut bg = egui::Color32::TRANSPARENT;
    let mut bold = false;
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\u{1b}' {
            push_run(&mut job, &buf, font_id, fg, bg);
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
                '\r' => buf.push('\n'), // 终端常见的 CR 按换行显示
                '\n' | '\t' => buf.push(ch),
                c if (c as u32) < 0x20 => {} // 其余控制字符不显示
                _ => buf.push(ch),
            }
            i += 1;
        }
    }
    push_run(&mut job, &buf, font_id, fg, bg);
    job
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
            dock_state: Some(DockState::new(UiPanel::ALL.to_vec())),
        }
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

        let output = ctx.run_ui(Default::default(), |ui| {
            app.receive_area(ui);
        });

        // 全选模式下应绘制出蓝色高亮（蓝底白字）
        let prims = ctx.tessellate(output.shapes, 1.0);
        let mut found = false;
        for p in prims {
            if let egui::epaint::Primitive::Mesh(mesh) = p.primitive
                && mesh.vertices.iter().any(|v| v.color == theme::BLUE_CHECK)
            {
                found = true;
                break;
            }
        }
        assert!(found, "全选模式下应绘制蓝色高亮");
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
    fn terminal_text_builds_without_timestamps() {
        let mut app = make_app();
        app.append_rx(b"hello\n");
        app.refresh_terminal_text();
        assert_eq!(app.terminal_text, "hello\n");
        assert!(!app.terminal_text.contains("[RX]"));
        assert!(!app.terminal_text.contains('['));
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
    fn terminal_area_renders_dark_bg_ansi_text_and_pending_input() {
        let mut app = make_app();
        app.config.terminal_mode = true;
        app.append_rx(b"\x1b[32mOK\x1b[0m\n");
        app.refresh_terminal_text();
        // 回车发送模式下，待发送输入行直接显示在终端里
        app.terminal_input = "AT".to_string();
        app.terminal_cursor = 2;
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
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
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("AT".to_string())],
                focused: true,
                ..Default::default()
            },
            |ui| app.handle_terminal_keys(ui),
        );
        assert_eq!(app.terminal_input, "AT");
        assert!(app.terminal_buffer.is_empty());

        // 回车发送整行 + CR；自动回显时同步显示
        let _ = ctx.run_ui(
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
        assert_eq!(app.terminal_buffer, b"AT\r");
    }

    #[test]
    fn terminal_immediate_keys_send_and_echo() {
        let mut app = make_app();
        app.config.terminal_auto_echo = true;
        app.config.terminal_enter_sends = false;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(
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
}
