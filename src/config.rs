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

impl Default for SendQueue {
    fn default() -> Self {
        Self::new("队列 1")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub port: PortSettings,
    /// 界面语言（缺省时按系统语言决定）
    #[serde(default = "default_language")]
    pub language: Language,
    pub display_mode: DisplayMode,
    pub text_encoding: TextEncoding,
    pub autoscroll: bool,
    /// 是否在数据展示区显示发送的数据
    #[serde(default = "default_true")]
    pub show_sent_data: bool,
    pub send_mode: SendMode,
    pub line_ending: LineEnding,
    pub periodic_interval_ms: u32,
    pub file_mode: FileSendMode,
    pub file_line_interval_ms: u32,
    pub queues: Vec<SendQueue>,
    pub selected_queue: usize,
    pub max_queue_items: usize,
    pub auto_refresh_ports: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: PortSettings::default(),
            language: Language::system(),
            display_mode: DisplayMode::Text,
            text_encoding: TextEncoding::Utf8,
            autoscroll: true,
            show_sent_data: true,
            send_mode: SendMode::Text,
            line_ending: LineEnding::None,
            periodic_interval_ms: 1000,
            file_mode: FileSendMode::WholeFile,
            file_line_interval_ms: 100,
            queues: vec![SendQueue::default()],
            selected_queue: 0,
            max_queue_items: 200,
            auto_refresh_ports: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_language() -> Language {
    Language::system()
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
                    if c.queues.is_empty() {
                        c.queues.push(SendQueue::default());
                    }
                    c.selected_queue = c.selected_queue.min(c.queues.len() - 1);
                    c.max_queue_items = c.max_queue_items.clamp(1, 1000);
                    c
                }
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
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
}
