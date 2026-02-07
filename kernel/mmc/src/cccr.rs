//! SDIO CCCR（Common I/O）访问与规范相关
//!
//! 对应 Linux drivers/mmc/core/sdio_ops.c 中通过 F0 访问 CCCR 的逻辑。
//! SDIO 规范：F0 0x02 = IO_ENABLE，0x03 = IO_READY；使能 function N 即置位 IO_ENABLE bit N 并轮询 IO_READY bit N。

/// CCCR 寄存器偏移（F0 地址）
pub const SDIO_CCCR_IO_ENABLE: u8 = 0x02;
pub const SDIO_CCCR_IO_READY: u8 = 0x03;

/// aic8800 D80/V3 等使用的 F0 扩展寄存器（LicheeRV sdio_f0_writeb 目标）
/// 仅作文档与常量统一用，实际写由 SdioFunc::write_f0 或 CccrAccess::write_f0 完成。
pub mod sdio_f0_reg {
    /// D80 aicwf_sdio_bus_start 等写 0x07 到 0x04
    pub const SDIO_F0_04: u8 = 0x04;
    /// feature.sdio_phase 写到 0x13（func_init 路径）
    pub const SDIO_F0_13: u8 = 0x13;
    /// V3 扩展 0xF0/0xF1/0xF2/0xF8
    pub const SDIO_F0_F0: u8 = 0xF0;
    pub const SDIO_F0_F1: u8 = 0xF1;
    pub const SDIO_F0_F2: u8 = 0xF2;
    pub const SDIO_F0_F8: u8 = 0xF8;
}

/// 能读写 F0（CCCR）的接口，用于使能/禁用 function
pub trait CccrAccess {
    /// 读 F0 寄存器（addr 0x00..=0xFF）
    fn read_f0(&self, reg: u8) -> Result<u8, i32>;
    /// 写 F0 寄存器
    fn write_f0(&self, reg: u8, val: u8) -> Result<(), i32>;
}

/// 延时回调（no_std 下由调用方提供，如 delay_spin_ms）
pub trait DelayMs {
    fn delay_ms(&mut self, ms: u32);
}

/// 按 SDIO 规范使能指定 function：写 IO_ENABLE bit，轮询 IO_READY 直至置位或超时
pub fn sdio_enable_function(
    cccr: &dyn CccrAccess,
    func_num: u8,
    timeout_ms: u32,
    delay: &mut dyn DelayMs,
) -> Result<(), i32> {
    if func_num == 0 || func_num > 7 {
        return Err(-22);
    }
    let bit = 1u8 << func_num;
    let io_enable = cccr.read_f0(SDIO_CCCR_IO_ENABLE)?;
    cccr.write_f0(SDIO_CCCR_IO_ENABLE, io_enable | bit)?;
    let mut waited = 0u32;
    const POLL_MS: u32 = 2;
    while waited < timeout_ms {
        delay.delay_ms(POLL_MS);
        waited += POLL_MS;
        let io_ready = cccr.read_f0(SDIO_CCCR_IO_READY).unwrap_or(0);
        if (io_ready & bit) != 0 {
            return Ok(());
        }
    }
    Err(-110)
}

/// 按 SDIO 规范禁用指定 function：清除 IO_ENABLE bit
pub fn sdio_disable_function(cccr: &dyn CccrAccess, func_num: u8) -> Result<(), i32> {
    if func_num == 0 || func_num > 7 {
        return Err(-22);
    }
    let bit = 1u8 << func_num;
    let io_enable = cccr.read_f0(SDIO_CCCR_IO_ENABLE)?;
    cccr.write_f0(SDIO_CCCR_IO_ENABLE, io_enable & !bit)
}
