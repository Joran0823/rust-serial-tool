// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 应用配置：端口参数、UI 偏好、发送队列等，序列化为 TOML 持久化。

use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    #[default]
    Eight,
}

impl DataBits {
    pub fn label(self) -> &'static str {
        match self {
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
        }
    }

    pub fn to_serial(self) -> serialport::DataBits {
        match self {
            Self::Five => serialport::DataBits::Five,
            Self::Six => serialport::DataBits::Six,
            Self::Seven => serialport::DataBits::Seven,
            Self::Eight => serialport::DataBits::Eight,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StopBits {
    #[default]
    One,
    OnePointFive,
    Two,
}

impl StopBits {
    pub fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::OnePointFive => "1.5",
            Self::Two => "2",
        }
    }

    pub fn to_serial(self, lang: Language) -> Result<serialport::StopBits, String> {
        match self {
            Self::One => Ok(serialport::StopBits::One),
            Self::Two => Ok(serialport::StopBits::Two),
            Self::OnePointFive => Err(if lang == Language::Chinese {
                "serialport 库暂不支持 1.5 停止位".to_string()
            } else {
                "1.5 stop bits are not supported by the serialport crate".to_string()
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
    Mark,
    Space,
}

impl Parity {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Even => "Even",
            Self::Odd => "Odd",
            Self::Mark => "Mark",
            Self::Space => "Space",
        }
    }

    pub fn to_serial(self, lang: Language) -> Result<serialport::Parity, String> {
        match self {
            Self::None => Ok(serialport::Parity::None),
            Self::Even => Ok(serialport::Parity::Even),
            Self::Odd => Ok(serialport::Parity::Odd),
            Self::Mark => Err(if lang == Language::Chinese {
                "serialport 库暂不支持 Mark 校验".to_string()
            } else {
                "Mark parity is not supported by the serialport crate".to_string()
            }),
            Self::Space => Err(if lang == Language::Chinese {
                "serialport 库暂不支持 Space 校验".to_string()
            } else {
                "Space parity is not supported by the serialport crate".to_string()
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FlowControl {
    #[default]
    None,
    Software,
    Hardware,
}

impl FlowControl {
    pub fn to_serial(self) -> serialport::FlowControl {
        match self {
            Self::None => serialport::FlowControl::None,
            Self::Software => serialport::FlowControl::Software,
            Self::Hardware => serialport::FlowControl::Hardware,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PortSettings {
    pub port_name: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub stop_bits: StopBits,
    pub parity: Parity,
    pub flow_control: FlowControl,
}

impl Default for PortSettings {
    fn default() -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            stop_bits: StopBits::One,
            parity: Parity::None,
            flow_control: FlowControl::None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DisplayMode {
    #[default]
    Text,
    Hex,
}

impl DisplayMode {
    pub fn label(self, lang: Language) -> &'static str {
        match lang {
            Language::Chinese => match self {
                Self::Text => "文本",
                Self::Hex => "HEX",
            },
            Language::English => match self {
                Self::Text => "Text",
                Self::Hex => "HEX",
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SendMode {
    #[default]
    Text,
    Hex,
}

impl SendMode {
    pub fn label(self, lang: Language) -> &'static str {
        match lang {
            Language::Chinese => match self {
                Self::Text => "文本",
                Self::Hex => "HEX",
            },
            Language::English => match self {
                Self::Text => "Text",
                Self::Hex => "HEX",
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(clippy::upper_case_acronyms)]
pub enum LineEnding {
    #[default]
    None,
    CR,
    LF,
    CRLF,
}

impl LineEnding {
    pub fn label(self, lang: Language) -> &'static str {
        match lang {
            Language::Chinese => match self {
                Self::None => "无",
                Self::CR => "CR",
                Self::LF => "LF",
                Self::CRLF => "CRLF",
            },
            Language::English => match self {
                Self::None => "None",
                Self::CR => "CR",
                Self::LF => "LF",
                Self::CRLF => "CRLF",
            },
        }
    }

    pub fn bytes(self) -> &'static [u8] {
        match self {
            Self::None => b"",
            Self::CR => b"\r",
            Self::LF => b"\n",
            Self::CRLF => b"\r\n",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Gbk,
    Ascii,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Gbk => "GBK",
            Self::Ascii => "ASCII",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FileSendMode {
    #[default]
    WholeFile,
    LineByLine,
}

impl FileSendMode {
    pub fn label(self, lang: Language) -> &'static str {
        match lang {
            Language::Chinese => match self {
                Self::WholeFile => "整块发送",
                Self::LineByLine => "逐行发送",
            },
            Language::English => match self {
                Self::WholeFile => "Whole file",
                Self::LineByLine => "Line by line",
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueueItem {
    pub mode: SendMode,
    pub content: String,
    /// 队列发送时，发送完该条目后等待的毫秒数；0 = 紧接着发送下一条。单条手动发送忽略。
    pub delay_ms: u32,
    /// 是否参与队列发送（队列发送时仅发送选中的条目）。新增条目默认选中。
    #[serde(default = "default_true")]
    pub selected: bool,
}

impl Default for QueueItem {
    fn default() -> Self {
        Self {
            mode: SendMode::Text,
            content: String::new(),
            delay_ms: 100,
            selected: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendQueue {
    pub name: String,
    pub items: Vec<QueueItem>,
}

impl SendQueue {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            items: vec![QueueItem::default()],
        }
    }
}


/// 界面主题设置：跟随系统 / 深色 / 亮色。
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeSetting {
    /// 跟随系统主题（启动时按系统深浅色）
    #[default]
    System,
    /// 始终深色
    Dark,
    /// 始终亮色
    Light,
}

impl ThemeSetting {
    /// 当前是否应使用深色（System 时按系统）。
    pub fn is_dark(&self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Dark => true,
            Self::Light => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub port: PortSettings,
    /// 界面语言（缺省时按系统语言决定）
    #[serde(default = "default_language")]
    pub language: Language,
    /// 界面主题（缺省跟随系统）
    #[serde(default)]
    pub theme: ThemeSetting,
    pub display_mode: DisplayMode,
    pub text_encoding: TextEncoding,
    pub autoscroll: bool,
    /// 是否在数据展示区显示时间戳（默认勾选）
    #[serde(default = "default_true")]
    pub show_timestamps: bool,
    pub send_mode: SendMode,
    pub line_ending: LineEnding,
    pub periodic_interval_ms: u32,
    pub file_mode: FileSendMode,
    pub file_line_interval_ms: u32,
    pub queues: Vec<SendQueue>,
    pub selected_queue: usize,
    pub max_queue_items: usize,
    pub auto_refresh_ports: bool,
    /// 接收区终端模式：深色背景、无时间戳、支持 ANSI 颜色，接收区可直接键盘输入
    #[serde(default = "default_false")]
    pub terminal_mode: bool,
    /// 本地回显：非终端模式下在展示区显示发送的数据（[TX] 标记），
    /// 终端模式下回显键盘输入内容（对端无回显时使用）；默认勾选
    #[serde(default = "default_true")]
    pub terminal_auto_echo: bool,
    /// 终端模式：仅按回车时发送整行（回车字符一并发送）；关闭后按键即发
    #[serde(default = "default_true")]
    pub terminal_enter_sends: bool,
}

impl Default for Config {
    fn default() -> Self {
        let language = Language::system();
        Self {
            port: PortSettings::default(),
            language,
            theme: ThemeSetting::System,
            display_mode: DisplayMode::Text,
            text_encoding: TextEncoding::Utf8,
            autoscroll: true,
            show_timestamps: true,
            send_mode: SendMode::Text,
            line_ending: LineEnding::None,
            periodic_interval_ms: 1000,
            file_mode: FileSendMode::WholeFile,
            file_line_interval_ms: 100,
            queues: vec![SendQueue::new(default_queue_name(language))],
            selected_queue: 0,
            max_queue_items: 200,
            auto_refresh_ports: true,
            terminal_mode: false,
            terminal_auto_echo: true,
            terminal_enter_sends: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_language() -> Language {
    Language::system()
}

/// 旧版本（V1.1.2 及更早）硬编码的默认队列名。
const LEGACY_DEFAULT_QUEUE_NAME: &str = "队列 1";

/// 按语言生成默认队列名，与界面“新建队列”按钮的命名一致。
fn default_queue_name(lang: Language) -> String {
    lang.strings()
        .fill(lang.strings().queue_new_name, &[("n", "1".to_string())])
}

impl Config {
    /// 配置文件路径：Windows %APPDATA%\serial-tool\config.toml
    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "serial-tool", "serial-tool")
            .map(|d| d.config_dir().join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<Config>(&s) {
                Ok(mut c) => {
                    c.normalize_queues();
                    c.selected_queue = c.selected_queue.min(c.queues.len() - 1);
                    c.max_queue_items = c.max_queue_items.clamp(1, 1000);
                    c
                }
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// 队列兜底与迁移：
    /// - 没有任何队列时按当前语言创建默认队列；
    /// - 旧版默认队列名是硬编码中文“队列 1”，英文界面下会显得界面语言混杂
    ///   （issue #6）。仅当队列仍未被使用（所有条目内容为空）时才改名为
    ///   当前语言的默认名，避免改动用户自定义数据。
    fn normalize_queues(&mut self) {
        if self.queues.is_empty() {
            self.queues
                .push(SendQueue::new(default_queue_name(self.language)));
            return;
        }
        if self.language != Language::Chinese {
            let default_name = default_queue_name(self.language);
            for q in &mut self.queues {
                if q.name == LEGACY_DEFAULT_QUEUE_NAME
                    && q.items.iter().all(|i| i.content.trim().is_empty())
                {
                    q.name = default_name.clone();
                }
            }
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let c = Config::default();
        let s = toml::to_string(&c).expect("serialize");
        let c2: Config = toml::from_str(&s).expect("deserialize");
        assert_eq!(c.port.baud_rate, c2.port.baud_rate);
        assert_eq!(c.language, c2.language);
        assert_eq!(c.queues[0].name, c2.queues[0].name);
        assert_eq!(c.queues[0].items[0].delay_ms, c2.queues[0].items[0].delay_ms);
    }

    #[test]
    fn default_line_ending_is_none() {
        assert_eq!(Config::default().line_ending, LineEnding::None);
        assert_eq!(LineEnding::default(), LineEnding::None);
    }

    #[test]
    fn terminal_defaults_and_old_config_compat() {
        let c = Config::default();
        assert!(!c.terminal_mode, "终端模式默认关闭");
        assert!(c.terminal_auto_echo, "自动回显默认勾选");
        assert!(c.terminal_enter_sends, "回车发送默认开启");

        // 旧版本配置文件没有终端字段，应能正常反序列化并取默认值
        let old = toml::from_str::<Config>(
            r#"
display_mode = "Text"
text_encoding = "Utf8"
autoscroll = true
send_mode = "Text"
line_ending = "None"
periodic_interval_ms = 1000
file_mode = "WholeFile"
file_line_interval_ms = 100
queues = []
selected_queue = 0
max_queue_items = 200
auto_refresh_ports = true

[port]
port_name = "COM3"
baud_rate = 9600
data_bits = "Eight"
stop_bits = "One"
parity = "None"
flow_control = "None"
"#,
        )
        .expect("旧版配置应能反序列化");
        assert!(!old.terminal_mode);
        assert!(old.terminal_auto_echo);
        assert!(old.terminal_enter_sends);
    }

    #[test]
    fn default_queue_name_is_localized() {
        assert_eq!(default_queue_name(Language::Chinese), "队列 1");
        assert_eq!(default_queue_name(Language::English), "Queue 1");
    }

    #[test]
    fn empty_queues_get_localized_default_on_load() {
        let mut c = Config {
            language: Language::English,
            queues: Vec::new(),
            ..Config::default()
        };
        c.normalize_queues();
        assert_eq!(c.queues.len(), 1);
        assert_eq!(c.queues[0].name, "Queue 1");
    }

    #[test]
    fn legacy_default_queue_is_renamed_for_english_ui() {
        // 旧版默认队列（中文“队列 1” + 空条目）在英文界面下自动改为本地化默认名
        let mut c = Config {
            language: Language::English,
            queues: vec![SendQueue::new("队列 1")],
            ..Config::default()
        };
        c.normalize_queues();
        assert_eq!(c.queues[0].name, "Queue 1");
    }

    #[test]
    fn user_defined_queue_is_not_renamed() {
        // 队列名恰好等于旧默认名但已有内容时不迁移
        let mut q = SendQueue::new("队列 1");
        q.items[0].content = "hello".to_string();
        let mut c = Config {
            language: Language::English,
            queues: vec![q],
            ..Config::default()
        };
        c.normalize_queues();
        assert_eq!(c.queues[0].name, "队列 1");
    }

    #[test]
    fn legacy_default_queue_kept_for_chinese_ui() {
        // 中文界面下旧默认名无需改动
        let mut c = Config {
            language: Language::Chinese,
            queues: vec![SendQueue::new("队列 1")],
            ..Config::default()
        };
        c.normalize_queues();
        assert_eq!(c.queues[0].name, "队列 1");
    }
}
