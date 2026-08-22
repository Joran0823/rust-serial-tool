//! 发送队列的文件保存 / 导入。
//!
//! 主格式采用 TOML（与配置文件一致，可读、类型化、天然支持数组结构）；
//! 同时支持 TXT 便捷导入（每行一条文本条目，忽略空行）。

use crate::config::{QueueItem, SendMode, SendQueue};

const MAX_ITEMS: usize = 1000;

/// 将队列序列化为 TOML 字符串。
pub fn export_queue_toml(queue: &SendQueue) -> String {
    toml::to_string_pretty(queue).unwrap_or_default()
}

/// 从 TOML 解析队列（含名称与条目）。
pub fn import_queue_toml(s: &str) -> Result<SendQueue, String> {
    let queue: SendQueue = toml::from_str(s).map_err(|e| format!("TOML 解析失败: {e}"))?;
    if queue.items.len() > MAX_ITEMS {
        return Err(format!("条目数超过上限 {MAX_ITEMS}"));
    }
    Ok(queue)
}

/// 从文本解析条目：每行一条文本条目，忽略空行。
pub fn import_queue_txt(s: &str) -> Vec<QueueItem> {
    s.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| QueueItem {
            mode: SendMode::Text,
            content: l.to_string(),
            delay_ms: 100,
            selected: true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_round_trip() {
        let mut q = SendQueue::new("测试队列");
        q.items.push(QueueItem {
            mode: SendMode::Hex,
            content: "01 0A FF".to_string(),
            delay_ms: 50,
            selected: true,
        });
        let s = export_queue_toml(&q);
        let q2 = import_queue_toml(&s).expect("parse");
        assert_eq!(q.name, q2.name);
        assert_eq!(q.items.len(), q2.items.len());
        assert_eq!(q.items[1].content, q2.items[1].content);
        assert_eq!(q.items[1].delay_ms, q2.items[1].delay_ms);
    }

    #[test]
    fn txt_import_skips_blank() {
        let items = import_queue_txt("hello\n\nworld\r\n  \nthird");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].content, "hello");
        assert_eq!(items[2].content, "third");
        assert!(items.iter().all(|i| i.mode == SendMode::Text));
    }
}
