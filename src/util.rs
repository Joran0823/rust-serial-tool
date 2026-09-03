// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Joran

//! 通用工具函数。

use chrono::Local;

/// 当前本地时间，格式 `HH:MM:SS.mmm`。
pub fn now_hms_ms() -> String {
    Local::now().format("%H:%M:%S%.3f").to_string()
}
 
