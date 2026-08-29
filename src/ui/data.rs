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
use crate::ui::theme;
use crate::util;
use eframe::egui;
use std::io::Write;

const DATA_DISPLAY_CAP: usize = 256 * 1024;
/// 显示缓存字符数上限（超出后丢弃最早的内容）
const DISPLAY_CACHE_CAP: usize = 300_000;
/// 接收区右键菜单“全选”的一次性标记键（存储于 egui 临时数据）
const RECV_SELECT_ALL_KEY: &str = "recv_select_all";

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
                .color(theme::TEXT_SOFT),
            );
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(s.receive_area).strong());
            if !self.config.show_sent_data {
                ui.label(
                    egui::RichText::new(s.not_showing_sent)
                        .small()
                        .color(theme::TEXT_SOFT),
                );
            }
        });
        ui.separator();
        // 只读展示区：无边框、浅灰背景（符合只读属性），不可编辑
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(0xF0, 0xF0, 0xF0))
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
                                    egui::RichText::new(s.receive_empty).color(theme::TEXT_SOFT),
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
                                        theme::TEXT
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
        self.display_cache.push_str(marker);
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

/// 用流式解码器解码字节并返回 UTF-8 字符串。
/// 使用足够大的输出缓冲，避免 `decode_to_string` 因 String 容量不足而截断。
fn stream_decode(dec: &mut encoding_rs::Decoder, bytes: &[u8], last: bool) -> String {
    let mut out = vec![0u8; bytes.len() * 3 + 16];
    let (_, read, written, _) = dec.decode_to_utf8(bytes, &mut out, last);
    debug_assert!(read == bytes.len(), "解码输出缓冲不足");
    String::from_utf8_lossy(&out[..written]).into_owned()
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
}
