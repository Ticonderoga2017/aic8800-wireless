//! 重导出 sdio::irq，供 cmd.rs、flow.rs 等使用 crate::sdio_irq::*（与 LicheeRV 程序逻辑对齐）

pub use crate::sdio::irq::{
    ensure_sdio_irq_registered, notify_bustx, notify_cmd_done, notify_sdio_irq_work,
    notify_sdio_irq_work_done, notify_tx_done, notify_wait_done,
    sdio_tick, set_use_soft_irq_wake, use_sdio_irq, wait_bustx_or_timeout, wait_cmd_done_timeout,
    wait_sdio_irq_work_done_timeout, wait_sdio_irq_work_or_timeout,
    wait_sdio_or_timeout, wait_tx_done_timeout, SDIO_TIMER_POLL_INTERVAL_MS,
};
