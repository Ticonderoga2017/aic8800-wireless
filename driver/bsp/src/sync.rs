//! BSP 同步原语（对应 aic_bsp_main.c 中 mutex_init、sema_init 两个原语，wireless 中均实现）
//!
//! - **power_lock**（spin::Mutex）：对应 mutex aicbsp_power_lock，保护 aicbsp_set_subsys 等电源/上下电序列
//! - **probe 完成信号**（AtomicBool + 超时轮询）：对应 semaphore aicbsp_probe_semaphore；aicbsp_sdio_init 用 down_timeout(2000) 等待，probe 里 up()

use core::sync::atomic::{AtomicBool, Ordering};

/// 电源/子系统互斥锁（对应 struct mutex aicbsp_power_lock）
static POWER_LOCK: spin::Mutex<()> = spin::Mutex::new(());

/// SDIO probe 完成标志（对应 semaphore aicbsp_probe_semaphore，sema_init(0)）
/// probe 成功时置 true，aicbsp_sdio_init 里等待 true 或超时
static PROBE_SIGNAL: AtomicBool = AtomicBool::new(false);

/// 重置 probe 信号（对应 sema_init(&aicbsp_probe_semaphore, 0)）
/// 在 aicbsp_init 时调用，保证每次 init 后需再次等待 probe
#[inline]
pub fn probe_reset() {
    PROBE_SIGNAL.store(false, Ordering::SeqCst);
}

/// 通知“SDIO probe 已完成”（对应 up(&aicbsp_probe_semaphore)）
#[inline]
pub fn probe_signal() {
    PROBE_SIGNAL.store(true, Ordering::SeqCst);
}

/// 忙等延时的每毫秒循环数（与 LicheeRV mdelay 对齐时使用，无精确时钟时为启发式近似）
pub const LOOPS_PER_MS: u32 = 1000;

/// 忙等约 ms 毫秒（对应 LicheeRV 的 mdelay(ms)）
/// 无标准时钟时时长与 CPU 频率相关，仅作与 LicheeRV 流程对齐的近似延时
#[inline]
pub fn delay_spin_ms(ms: u32) {
    let limit = ms.saturating_mul(LOOPS_PER_MS);
    for _ in 0..limit {
        core::hint::spin_loop();
    }
}

/// 忙等约 us 微秒（对应 LicheeRV 的 udelay(us)，如 aicwf_sdio_func_init 中 enable_func 后 udelay(100)）
#[inline]
pub fn delay_spin_us(us: u32) {
    let limit = us.saturating_mul(LOOPS_PER_MS) / 1000; // 1000us=1ms => us 微秒 ≈ us*LOOPS_PER_MS/1000 次循环
    for _ in 0..limit.max(1) {
        core::hint::spin_loop();
    }
}

/// 等待“SDIO probe 已完成”，最多约 timeout_ms 毫秒（对应 down_timeout(..., msecs_to_jiffies(2000))）
/// 无标准时钟时用忙等轮询，时长与 CPU 频率相关；返回 Ok(()) 表示已收到信号，Err(()) 表示超时
pub fn probe_wait_timeout_ms(timeout_ms: u32) -> Result<(), ()> {
    let limit = timeout_ms.saturating_mul(LOOPS_PER_MS);
    for _ in 0..limit {
        if PROBE_SIGNAL.load(Ordering::SeqCst) {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(())
}

/// 获取电源锁（对应 mutex_lock(&aicbsp_power_lock)）
/// 返回 guard，析构时自动释放
#[inline]
pub fn power_lock() -> spin::MutexGuard<'static, ()> {
    POWER_LOCK.lock()
}
