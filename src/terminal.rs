// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 基于 `vt100` 的终端屏幕模型。
//!
//! 行首时间戳作为真正的终端内容写入屏幕；CR/LF/光标移动全部交给
//! `vt100` 处理，避免在字符串层面模拟终端。

use crate::ui::theme;
use eframe::egui;
use std::num::NonZeroU16;

/// 虚拟终端高度。超出后由 vt100 滚动，旧行被丢弃。
pub const ROWS: u16 = 2048;
/// 虚拟终端宽度。超长行由 vt100 自动换行。
pub const COLS: u16 = 240;

const ANSI_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x7A, 0x7A, 0x7A),
    egui::Color32::from_rgb(0xCD, 0x31, 0x31),
    egui::Color32::from_rgb(0x0D, 0xBC, 0x79),
    egui::Color32::from_rgb(0xE5, 0xE5, 0x10),
    egui::Color32::from_rgb(0x24, 0x72, 0xC8),
    egui::Color32::from_rgb(0xBC, 0x3F, 0xBC),
    egui::Color32::from_rgb(0x11, 0xA8, 0xCD),
    egui::Color32::from_rgb(0xE5, 0xE5, 0xE5),
];

const ANSI_BRIGHT_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0x66, 0x66, 0x66),
    egui::Color32::from_rgb(0xF1, 0x4C, 0x4C),
    egui::Color32::from_rgb(0x23, 0xD1, 0x8B),
    egui::Color32::from_rgb(0xF5, 0xF5, 0x43),
    egui::Color32::from_rgb(0x3B, 0x8E, 0xEA),
    egui::Color32::from_rgb(0xD6, 0x70, 0xD6),
    egui::Color32::from_rgb(0x29, 0xB8, 0xDB),
    egui::Color32::from_rgb(0xFF, 0xFF, 0xFF),
];

pub struct TerminalScreen {
    parser: vt100::Parser,
    initialized: bool,
    skip_lf: bool,
    ignore_leading_newline: bool,
}

impl TerminalScreen {
    #[must_use]
    pub fn new() -> Self {
        Self {
            parser: vt100::Parser::new(
                NonZeroU16::new(ROWS).expect("ROWS > 0"),
                NonZeroU16::new(COLS).expect("COLS > 0"),
                0,
            ),
            initialized: false,
            skip_lf: false,
            ignore_leading_newline: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 首次进入终端模式时写入当前行的时间戳前缀。
    pub fn ensure_initial_prefix(&mut self, prefix: &str) {
        if !self.initialized {
            self.write_prefix(prefix);
            self.initialized = true;
        }
    }

    /// 接收数据显示到终端。换行统一按 CRLF 处理；每个换行后的新行
    /// 都写入 `prefix`，这样时间戳会保留在历史行中。
    pub fn feed_received(&mut self, text: &str, prefix: &str) {
        if text.is_empty() {
            return;
        }
        if !self.initialized {
            self.ensure_initial_prefix(prefix);
        }
        self.ensure_line_prefix(prefix);

        let mut rest = text;
        if self.ignore_leading_newline {
            self.ignore_leading_newline = false;
            if let Some(stripped) = rest.strip_prefix("\r\n") {
                rest = stripped;
            } else if let Some(stripped) = rest.strip_prefix('\r') {
                rest = stripped;
                self.skip_lf = true;
            } else if let Some(stripped) = rest.strip_prefix('\n') {
                rest = stripped;
            }
            if rest.is_empty() {
                return;
            }
        }
        while !rest.is_empty() {
            if self.skip_lf {
                if let Some(stripped) = rest.strip_prefix('\n') {
                    rest = stripped;
                    self.skip_lf = false;
                    continue;
                }
                self.skip_lf = false;
            }

            let Some(pos) = rest.find(['\r', '\n']) else {
                self.parser.process(rest.as_bytes());
                break;
            };
            let segment = &rest[..pos];
            if !segment.is_empty() {
                self.parser.process(segment.as_bytes());
            }
            let ch = rest[pos..].chars().next().expect("换行字符存在");
            // Linux tty 的 ONLCR/ICRNL 行为：CR、LF、CRLF 都作为一次换行。
            self.parser.process(b"\r\n");
            self.skip_lf = ch == '\r';
            rest = &rest[pos + ch.len_utf8()..];
            self.write_prefix(prefix);
        }
    }

    /// 本地回显（不含换行）。
    pub fn feed_local(&mut self, text: &str) {
        if !text.is_empty() {
            self.parser.process(text.as_bytes());
        }
    }

    /// 本地回车换行：写入一次 CRLF 并开始新的带前缀行。
    pub fn feed_local_line_break(&mut self, prefix: &str) {
        self.parser.process(b"\r\n");
        self.write_prefix(prefix);
        self.skip_lf = false;
        // 对端通常会回显我们刚发出的回车；下一次收到的首个换行视为
        // 本地回显的回声，不再额外制造一行空行。
        self.ignore_leading_newline = true;
    }

    #[cfg(test)]
    #[must_use]
    pub fn cursor(&self) -> (u16, u16) {
        self.parser.screen().cursor_position()
    }

    /// 终端纯文本内容（用于复制和测试）。
    #[must_use]
    pub fn plain_text(&self) -> String {
        let screen = self.parser.screen();
        let cursor_row = screen.cursor_position().0;
        let mut out = String::new();
        for row in 0..=cursor_row {
            if let Some(last) = last_used_col(screen, row) {
                for col in 0..=last {
                    if let Some(cell) = screen.cell(row, col) {
                        if cell.is_wide_continuation() {
                            continue;
                        }
                        if cell.has_contents() {
                            out.push_str(cell.contents());
                        } else {
                            out.push(' ');
                        }
                    }
                }
            }
            if row < cursor_row && !screen.row_wrapped(row) {
                out.push('\n');
            }
        }
        out
    }

    /// 把终端屏幕追加到 egui 的 LayoutJob。
    pub fn append_job(&self, job: &mut egui::text::LayoutJob, font_id: &egui::FontId) {
        let screen = self.parser.screen();
        let cursor_row = screen.cursor_position().0;
        for row in 0..=cursor_row {
            let Some(last) = last_used_col(screen, row) else {
                if row < cursor_row && !screen.row_wrapped(row) {
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
                continue;
            };
            let mut run = String::new();
            let mut current: Option<CellFormat> = None;

            for col in 0..=last {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let format = CellFormat::from_cell(cell);
                if current != Some(format) {
                    push_run(job, &run, font_id, current);
                    run.clear();
                    current = Some(format);
                }
                if cell.has_contents() {
                    run.push_str(cell.contents());
                } else {
                    run.push(' ');
                }
            }
            push_run(job, &run, font_id, current);

            if row < cursor_row && !screen.row_wrapped(row) {
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
        }
    }

    fn ensure_line_prefix(&mut self, prefix: &str) {
        if self.parser.screen().cursor_position().1 == 0 {
            self.write_prefix(prefix);
        }
    }

    fn write_prefix(&mut self, prefix: &str) {
        if prefix.is_empty() {
            return;
        }
        // 使用主题绿色（truecolor），只作用于时间戳/#，不影响后续数据。
        let color = theme::TERMINAL_PROMPT;
        let ansi = format!(
            "\x1b[38;2;{};{};{}m{prefix}\x1b[0m",
            color.r(),
            color.g(),
            color.b()
        );
        self.parser.process(ansi.as_bytes());
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CellFormat {
    fg: egui::Color32,
    bg: egui::Color32,
    italics: bool,
    underline: bool,
}

impl CellFormat {
    fn from_cell(cell: &vt100::Cell) -> Self {
        let bold = cell.bold();
        let mut fg = map_color(cell.fgcolor(), theme::TERMINAL_TEXT, bold);
        let mut bg = match cell.bgcolor() {
            vt100::Color::Default => egui::Color32::TRANSPARENT,
            color => map_color(color, egui::Color32::TRANSPARENT, false),
        };
        if cell.inverse() {
            let fg_solid = if fg == egui::Color32::TRANSPARENT {
                theme::TERMINAL_TEXT
            } else {
                fg
            };
            let bg_solid = if bg == egui::Color32::TRANSPARENT {
                theme::TERMINAL_BG
            } else {
                bg
            };
            fg = bg_solid;
            bg = fg_solid;
        }
        Self {
            fg,
            bg,
            italics: cell.italic(),
            underline: cell.underline(),
        }
    }
}

fn push_run(
    job: &mut egui::text::LayoutJob,
    run: &str,
    font_id: &egui::FontId,
    format: Option<CellFormat>,
) {
    if run.is_empty() {
        return;
    }
    let Some(format) = format else {
        return;
    };
    job.append(
        run,
        0.0,
        egui::TextFormat {
            font_id: font_id.clone(),
            color: format.fg,
            background: format.bg,
            italics: format.italics,
            underline: if format.underline {
                egui::Stroke::new(1.0, format.fg)
            } else {
                egui::Stroke::NONE
            },
            ..Default::default()
        },
    );
}

fn last_used_col(screen: &vt100::Screen, row: u16) -> Option<u16> {
    let mut last = None;
    for col in 0..COLS {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if !cell.has_contents() && is_default_cell(cell) {
            continue;
        }
        if (cell.has_contents() && cell.contents() != " ") || !is_default_cell(cell) {
            last = Some(col);
        }
    }
    last
}

fn is_default_cell(cell: &vt100::Cell) -> bool {
    cell.fgcolor() == vt100::Color::Default
        && cell.bgcolor() == vt100::Color::Default
        && !cell.bold()
        && !cell.italic()
        && !cell.underline()
        && !cell.inverse()
}

fn map_color(color: vt100::Color, default: egui::Color32, bold: bool) -> egui::Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(n) => {
            if n < 8 {
                if bold {
                    ANSI_BRIGHT_COLORS[usize::from(n)]
                } else {
                    ANSI_COLORS[usize::from(n)]
                }
            } else if n < 16 {
                ANSI_BRIGHT_COLORS[usize::from(n - 8)]
            } else if n < 232 {
                let n = n - 16;
                let r = n / 36;
                let g = (n % 36) / 6;
                let b = n % 6;
                let scale = |v: u8| -> u8 {
                    if v == 0 {
                        0
                    } else {
                        55 + v * 40
                    }
                };
                egui::Color32::from_rgb(scale(r), scale(g), scale(b))
            } else {
                let v = 8 + (n - 232) * 10;
                egui::Color32::from_rgb(v, v, v)
            }
        }
        vt100::Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(screen: &TerminalScreen) -> String {
        screen.plain_text()
    }

    #[test]
    fn crlf_only_leaves_prompt_then_cursor_on_next_line() {
        let mut screen = TerminalScreen::new();
        screen.ensure_initial_prefix("[00:00:00.000]# ");
        screen.feed_received("\r\n", "[00:00:01.000]# ");
        let text = plain(&screen);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "[00:00:00.000]# ");
        assert_eq!(lines[1], "[00:00:01.000]# ");
        assert_eq!(screen.cursor().0, 1);
    }

    #[test]
    fn trailing_crlf_is_not_rendered_and_cursor_moves_down() {
        let mut screen = TerminalScreen::new();
        screen.ensure_initial_prefix("[00:00:00.000]# ");
        screen.feed_received("OK\r\n", "[00:00:01.000]# ");
        let text = plain(&screen);
        assert!(text.contains("OK"));
        assert!(!text.contains('\r'));
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "[00:00:00.000]# OK");
        assert_eq!(lines[1], "[00:00:01.000]# ");
        assert_eq!(screen.cursor(), (1, 16));
    }

    #[test]
    fn split_crlf_across_batches_creates_one_line_break() {
        let mut screen = TerminalScreen::new();
        screen.ensure_initial_prefix("# ");
        screen.feed_received("OK\r", "# ");
        screen.feed_received("\nNEXT", "# ");
        let text = plain(&screen);
        assert_eq!(text, "# OK\n# NEXT");
    }

    #[test]
    fn bare_cr_and_lf_both_advance_to_next_line() {
        let mut screen = TerminalScreen::new();
        screen.ensure_initial_prefix("# ");
        screen.feed_received("A\rB\nC", "# ");
        assert_eq!(plain(&screen), "# A\n# B\n# C");
    }
}
