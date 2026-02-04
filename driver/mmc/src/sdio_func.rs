//! SDIO Function 抽象
//!
//! 对应 Linux：include/linux/mmc/sdio_func.h、drivers/mmc/core/sdio_ops.c。
//! aic8800 依赖：sdio_claim_host/release_host、sdio_readb/writeb、sdio_readsb/writesb、
//! sdio_set_block_size、sdio_enable_func、sdio_disable_func、sdio_claim_irq/release_irq。

use crate::types::{SdioDeviceId, SDIO_FUNC_BLOCKSIZE_DEFAULT};

/// SDIO 中断回调（对应 sdio_irq_handler_t）
/// 无 PLIC 时可由软中断/轮询替代调用。
pub type SdioIrqHandler = fn();

/// SDIO Function 设备接口
///
/// 对应 Linux struct sdio_func 及 sdio_* 系列 API。
/// 所有 I/O 在调用前应由调用方或实现方保证已 claim_host。
pub trait SdioFunc {
    /// Function 号（1..7）
    fn num(&self) -> u8;

    /// 厂商 ID（FBR）
    fn vendor(&self) -> u16;

    /// 设备 ID（FBR）
    fn device(&self) -> u16;

    /// 标准接口类
    fn class(&self) -> u8;

    /// 当前块大小（cur_blksize）
    fn cur_blksize(&self) -> u16;

    /// 单字节读（CMD52），addr 为 function 内寄存器偏移
    fn readb(&self, addr: u32) -> Result<u8, i32>;

    /// 单字节写（CMD52）
    fn writeb(&self, addr: u32, b: u8) -> Result<(), i32>;

    /// F0（CCCR）单字节读（对应 Linux sdio_f0_readb）
    /// reg 为 F0 内偏移 0x00..=0xFF。默认返回 Err(-38)(ENOSYS)，实现方可选提供。
    fn read_f0(&self, _reg: u8) -> Result<u8, i32> {
        Err(-38)
    }

    /// F0（CCCR）单字节写（对应 Linux sdio_f0_writeb）
    /// reg 为 F0 内偏移 0x00..=0xFF。默认返回 Err(-38)(ENOSYS)，实现方可选提供。
    fn write_f0(&self, _reg: u8, _val: u8) -> Result<(), i32> {
        Err(-38)
    }

    /// 块读（CMD53），addr 为 function 内起始偏移
    fn readsb(&self, addr: u32, buf: &mut [u8]) -> Result<usize, i32>;

    /// 块写（CMD53）
    fn writesb(&self, addr: u32, buf: &[u8]) -> Result<usize, i32>;

    /// 设置块大小（对应 sdio_set_block_size）
    fn set_block_size(&self, blksz: u16) -> Result<(), i32>;

    /// 使能该 function（CCCR IO_ENABLE + 等 IO_READY）
    fn enable_func(&self) -> Result<(), i32>;

    /// 关闭该 function
    fn disable_func(&self) -> Result<(), i32>;

    /// 注册 SDIO 中断（对应 sdio_claim_irq）。无 PLIC 时可为空实现，由软中断/轮询替代。
    fn claim_irq(&self, _handler: SdioIrqHandler) -> Result<(), i32> {
        Ok(())
    }

    /// 释放 SDIO 中断（对应 sdio_release_irq）
    fn release_irq(&self) -> Result<(), i32> {
        Ok(())
    }

    /// 对齐长度（对应 sdio_align_size，用于 512 块尾等）
    fn align_size(&self, sz: usize) -> usize {
        let blk = self.cur_blksize() as usize;
        if blk == 0 {
            return sz;
        }
        (sz + blk - 1) / blk * blk
    }

    /// 组成设备 ID（用于驱动匹配）
    fn device_id(&self) -> SdioDeviceId {
        SdioDeviceId::new(self.class(), self.vendor(), self.device())
    }
}

/// 默认块大小（与 aic8800 SDIOWIFI_FUNC_BLOCKSIZE 一致）
pub const fn default_func_blocksize() -> u16 {
    SDIO_FUNC_BLOCKSIZE_DEFAULT
}
