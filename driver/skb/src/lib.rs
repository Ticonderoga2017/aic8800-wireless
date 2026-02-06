//! Socket buffer (skb) 模块 — 对应 LicheeRV aic8800 依赖的 `linux/skbuff.h`
//!
//! 提供与 Linux `struct sk_buff` 语义对齐的包缓冲与队列，便于 BSP/FDRV 收发包路径与 LicheeRV 对照。
//!
//! - **[SkBuff]**：单包缓冲，`data`/`len`/`headroom`/`tailroom`、`put`/`pull`/`push`/`reserve`
//! - **[SkbQueue]**：FIFO 队列（对应 `struct sk_buff_head`），用于 RX 帧队列、TX 聚合等

#![no_std]

extern crate alloc;

mod queue;
mod skbuff;

pub use queue::SkbQueue;
pub use skbuff::SkBuff;
