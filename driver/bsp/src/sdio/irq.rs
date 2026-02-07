//! SDIO 等待与唤醒：支持 **PLIC 硬件中断** 与软中断（定时器 tick），与 LicheeRV 程序逻辑对齐
//!
//! 硬件路径：SDIO 卡有数据 → PLIC 触发 → sdio_irq_handler → notify_one → busrx 自 wait_sdio_or_timeout 唤醒 → run_poll_rx_one。
//! 软中断路径：定时器 tick → sdio_tick() → notify_one → 同上。
//!
//! 若未启用 PLIC（sdmmc_irq()==0）且未 set_use_soft_irq_wake(true)，busrx 以轮询 + sleep(RX_POLL_MS) 方式运行。

use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use axtask::WaitQueue;
use spin::Once;

use axhal::irq::register as irq_register;

/// SG2002 SD1（WiFi SD）在 PLIC 上的外设中断号。DTS：wifi-sd@4320000 interrupts = <38>；cv-sd@4310000 为 36，本 BSP 使用 0x4320000 故用 38。
const SD1_PLIC_IRQ: usize = 38;

/// 定时器驱动模式下，替代 SDIO IRQ 的轮询周期（毫秒）。与 LicheeRV 的 process_rxframes 调用频率对齐。
pub const SDIO_TIMER_POLL_INTERVAL_MS: u64 = 1;

/// SD1 的 PLIC 外设 IRQ 号；为 0 表示不使用 PLIC 硬件中断。
#[inline(always)]
fn sdmmc_irq() -> usize {
    SD1_PLIC_IRQ
}

/// 是否已启用软中断唤醒（定时器 tick 调用 sdio_tick 后 notify，busrx 在 WaitQueue 上阻塞等待）。
static USE_SOFT_IRQ_WAKE: AtomicBool = AtomicBool::new(false);

/// 软中断/定时器 tick 或 PLIC handler 通过 notify_one 唤醒本队列。
static SDIO_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// 仅执行一次：向 axhal 注册 SDIO 硬件 IRQ handler。
static SDIO_IRQ_REGISTER_ONCE: Once<()> = Once::new();

/// 主线程等待 CFM 完成时阻塞的队列：busrx 在 on_cfm 时 notify，主线程从 wait_cmd_done_or_timeout 唤醒（与 LicheeRV complete(&cmd->complete) 一致）
static CMD_DONE_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// bustx 线程等待“有发送任务”的队列：调用方 submit 后 notify_bustx()，与 LicheeRV complete(&bus_if->bustx_trgg) 一致
static BUSTX_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// 调用方等待“CMD 已写入 WR_FIFO”的队列：bustx 在 send_msg 完成后 notify_tx_done()，与 LicheeRV wake_up(&cmd_txdone_wait) 一致
static TX_DONE_WAIT_QUEUE: WaitQueue = WaitQueue::new();

/// PLIC 触发的 SDIO 卡中断：唤醒 busrx，由 busrx 执行 run_poll_rx_one 读 BLOCK_CNT/RD_FIFO（与 LicheeRV aicwf_sdio_hal_irqhandler → complete(busrx_trgg) 一致）。
fn sdio_irq_handler() {
    SDIO_WAIT_QUEUE.notify_one(false);
}

/// 软中断 tick：由平台定时器中断链（on_timer_tick → register_timer_callback）调用，notify 唤醒 busrx，对齐 LicheeRV 的 complete(&bus_if->busrx_trgg)。
#[inline]
pub fn sdio_tick() {
    SDIO_WAIT_QUEUE.notify_one(false);
}

/// 启用/关闭软中断唤醒。启用后应用需先调用 `axtask::register_timer_callback(|_| wireless::bsp::sdio_tick)`。
#[inline]
pub fn set_use_soft_irq_wake(enabled: bool) {
    USE_SOFT_IRQ_WAKE.store(enabled, Ordering::Release);
}

/// 若 sdmmc_irq() != 0，则向 axhal 注册 SDIO 硬件 IRQ handler（仅注册一次）。与 LicheeRV 的 claim_irq 语义一致。
pub fn ensure_sdio_irq_registered() {
    if sdmmc_irq() == 0 {
        return;
    }
    SDIO_IRQ_REGISTER_ONCE.call_once(|| {
        let ok = irq_register(SD1_PLIC_IRQ, sdio_irq_handler);
        if !ok {
            log::warn!(
                target: "wireless::bsp::sdio::irq",
                "ensure_sdio_irq_registered: axhal::irq::register(SD1_PLIC_IRQ={}) failed",
                SD1_PLIC_IRQ
            );
        }
    });
}

/// 是否使用“中断”唤醒（PLIC 外设 IRQ 或 软中断 tick）。为 true 时 busrx 用 wait_sdio_or_timeout 阻塞，与 LicheeRV 一致。
#[inline]
pub fn use_sdio_irq() -> bool {
    sdmmc_irq() != 0 || USE_SOFT_IRQ_WAKE.load(Ordering::Acquire)
}

/// 等待最多 `dur`，或直到 SDMMC IRQ 或 on_cfm 唤醒。若未使用 IRQ 则直接返回（由调用方 delay_spin_ms）
pub fn wait_sdio_or_timeout(dur: Duration) {
    if !use_sdio_irq() {
        return;
    }
    let _ = SDIO_WAIT_QUEUE.wait_timeout(dur);
}

/// RX 路径在 on_cfm 时调用，用于唤醒正在 wait_done 的线程（对齐 LicheeRV 的 complete(&cmd->complete)）。
/// 若存在专用 RX 线程，on_cfm 在彼处执行时调用本函数可使 wait_for_completion 的发送方立即返回。
pub fn notify_wait_done() {
    SDIO_WAIT_QUEUE.notify_one(false);
}

/// 通知“某条命令已完成”（由 busrx 在 on_cfm 后调用），唤醒正在 wait_cmd_done_or_timeout 阻塞的主线程。
/// 任务交给后台 busrx 线程静默执行，返回结果时通过本函数中断式通知主线程。
pub fn notify_cmd_done() {
    CMD_DONE_WAIT_QUEUE.notify_one(true);
}

/// 主线程阻塞等待“命令完成”或超时。若在 dur 内收到 notify_cmd_done 则立即返回 false（表示被唤醒）；
/// 若超时返回 true。主线程应循环：若 condition() 则返回 Ok；否则 wait_cmd_done_timeout(1ms)；超时则 Err。
pub fn wait_cmd_done_timeout(dur: Duration) -> bool {
    CMD_DONE_WAIT_QUEUE.wait_timeout(dur)
}

// ---------- bustx 线程（与 LicheeRV aicwf_sdio_bustx_thread 对齐）----------

/// 通知 bustx 线程“有发送任务”，与 LicheeRV complete(&bus_if->bustx_trgg) 一致。
#[inline]
pub fn notify_bustx() {
    BUSTX_WAIT_QUEUE.notify_one(false);
}

/// bustx 线程阻塞等待“有任务”或超时。若在 dur 内被 notify_bustx 唤醒则返回 false；超时返回 true。
pub fn wait_bustx_or_timeout(dur: Duration) -> bool {
    BUSTX_WAIT_QUEUE.wait_timeout(dur)
}

/// bustx 在 send_msg 完成后调用，唤醒正在 wait_tx_done_timeout 的调用方，与 LicheeRV wake_up(&cmd_txdone_wait) 一致。
#[inline]
pub fn notify_tx_done() {
    TX_DONE_WAIT_QUEUE.notify_one(true);
}

/// 调用方阻塞等待“CMD 已写入 WR_FIFO”或超时。若在 dur 内被 notify_tx_done 唤醒则返回 false；超时返回 true。
pub fn wait_tx_done_timeout(dur: Duration) -> bool {
    TX_DONE_WAIT_QUEUE.wait_timeout(dur)
}
