//! 串口会话层：封装 serialport，提供命令/事件通道。

pub mod session;

pub use session::{Command, Event, QueueSendItem, SerialSession};
