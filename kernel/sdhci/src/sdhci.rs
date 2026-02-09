//! LicheeRV writesb/readsb 底层依赖：Linux SDHCI 与 SDIO 层原样移植
//!
//! 调用链（LicheeRV-Nano-Build/linux_5.10）：
//!   sdio_writesb / sdio_readsb (sdio_io.c)
//!   → sdio_io_rw_ext_helper (sdio_io.c)
//!   → mmc_io_rw_extended (sdio_ops.c)
//!   → mmc_wait_for_req → host->ops->request = **sdhci_request** (sdhci.c)
//!   → sdhci_send_command → sdhci_prepare_data / ARGUMENT / sdhci_set_transfer_mode / COMMAND
//!
//! 本模块提供与 LicheeRV Linux 完全一致的寄存器定义、arg 计算与下发顺序，供 wireless BSP backend 使用。

// =============================================================================
// Linux drivers/mmc/host/sdhci.h - 控制器寄存器与常量
// =============================================================================

/// SDHCI 寄存器偏移（与 Linux sdhci.h 一致；SG2002 合并/映射见注释）
pub mod regs {
    /// DMA 地址 → SG2002 SDMA_SADDR (0x00)
    pub const SDHCI_DMA_ADDRESS: usize = 0x00;
    /// 块大小（11:0）→ SG2002 BLK_SIZE_AND_CNT 低 16 位 (0x04)
    pub const SDHCI_BLOCK_SIZE: usize = 0x04;
    /// 块数（15:0）→ SG2002 BLK_SIZE_AND_CNT 高 16 位 (0x06 与 0x04 合并为 32bit BLK_SIZE_AND_CNT)
    pub const SDHCI_BLOCK_COUNT: usize = 0x06;
    /// CMD 参数 → SG2002 ARGUMENT (0x08)
    pub const SDHCI_ARGUMENT: usize = 0x08;
    /// 传输模式 → SG2002 XFER_MODE_AND_CMD 低 16 位 (0x0C)
    pub const SDHCI_TRANSFER_MODE: usize = 0x0C;
    /// 命令 → SG2002 XFER_MODE_AND_CMD 高 16 位 (0x0E 与 0x0C 合并)
    pub const SDHCI_COMMAND: usize = 0x0E;
    /// 数据端口（PIO 时读写）→ SG2002 BUF_DATA (0x20)
    pub const SDHCI_BUFFER: usize = 0x20;
    /// 当前状态（CMD_INHIBIT 等）→ SG2002 PRESENT_STS (0x24)
    pub const SDHCI_PRESENT_STATE: usize = 0x24;
    /// 主机控制（DMA_SEL 等）→ SG2002 HOST_CTRL1 (0x28)
    pub const SDHCI_HOST_CONTROL: usize = 0x28;
    /// 超时控制 → SG2002 无独立 8bit，TRM 有 TIMEOUT_CTRL(0x2E) 等
    pub const SDHCI_TIMEOUT_CONTROL: usize = 0x2E;
    /// 中断状态 → SG2002 NORM_AND_ERR_INT_STS (0x30)
    pub const SDHCI_INT_STATUS: usize = 0x30;
    pub const SDHCI_INT_ENABLE: usize = 0x34;
    pub const SDHCI_RESPONSE: usize = 0x10;
}

/// SDHCI_MAKE_BLKSZ(sdma_boundary, blksz) — sdhci.c sdhci_set_block_info 用
/// Linux: ((dma & 0x7) << 12) | (blksz & 0xFFF)
#[inline(always)]
pub fn sdhci_make_blksz(sdma_boundary: u8, blksz: u16) -> u16 {
    ((u16::from(sdma_boundary) & 0x7) << 12) | (blksz & 0xFFF)
}

/// SDHCI_MAKE_CMD(cmd, flags) — sdhci.c sdhci_send_command 用
#[inline(always)]
pub fn sdhci_make_cmd(opcode: u8, flags: u8) -> u16 {
    (u16::from(opcode) << 8) | u16::from(flags & 0xFF)
}

/// 传输模式位（SDHCI_TRANSFER_MODE 寄存器）
pub mod trns {
    pub const SDHCI_TRNS_DMA: u16 = 0x01;
    pub const SDHCI_TRNS_BLK_CNT_EN: u16 = 0x02;
    pub const SDHCI_TRNS_AUTO_CMD12: u16 = 0x04;
    pub const SDHCI_TRNS_AUTO_CMD23: u16 = 0x08;
    pub const SDHCI_TRNS_READ: u16 = 0x10;
    pub const SDHCI_TRNS_MULTI: u16 = 0x20;
}

/// 命令响应/数据标志（sdhci_send_command 中 MAKE_CMD 的 flags）
pub mod cmd_flags {
    pub const SDHCI_CMD_RESP_NONE: u8 = 0x00;
    pub const SDHCI_CMD_RESP_SHORT: u8 = 0x02;  // R5
    pub const SDHCI_CMD_CRC: u8 = 0x08;
    pub const SDHCI_CMD_INDEX: u8 = 0x10;
    pub const SDHCI_CMD_DATA: u8 = 0x20;
}

/// HOST_CONTROL DMA 选择（与 sdhci.c sdhci_config_dma 一致）
pub mod host_ctrl_dma {
    pub const SDHCI_CTRL_SDMA: u8 = 0x00;  // SDMA
}

/// 中断状态位（SDHCI_INT_STATUS / NORM_AND_ERR_INT_STS）
pub mod int_bits {
    pub const SDHCI_INT_RESPONSE: u32 = 0x0000_0001;
    pub const SDHCI_INT_DATA_END: u32 = 0x0000_0002;
    pub const SDHCI_INT_DMA_END: u32 = 0x0000_0008;
    pub const SDHCI_INT_SPACE_AVAIL: u32 = 0x0000_0010;
    pub const SDHCI_INT_DATA_AVAIL: u32 = 0x0000_0020;
    pub const SDHCI_INT_TIMEOUT: u32 = 0x0001_0000;
    pub const SDHCI_INT_DATA_TIMEOUT: u32 = 0x0010_0000;
    pub const SDHCI_INT_DATA_CRC: u32 = 0x0020_0000;
    pub const SDHCI_INT_DATA_END_BIT: u32 = 0x0040_0000;
    pub const SDHCI_INT_ADMA_ERROR: u32 = 0x0200_0000;
}

/// PRESENT_STATE 位
pub mod present_state {
    pub const SDHCI_CMD_INHIBIT: u32 = 0x0000_0001;
    pub const SDHCI_DATA_INHIBIT: u32 = 0x0000_0002;
    pub const SDHCI_SPACE_AVAILABLE: u32 = 0x0000_0400;
    pub const SDHCI_DATA_AVAILABLE: u32 = 0x0000_0800;
}

/// R5 响应错误位（Linux include/linux/mmc/sdio.h），用于 mmc_io_rw_extended 后检查 cmd.resp[0]
pub mod r5_error {
    pub const R5_ERROR: u32 = 1 << 11;
    pub const R5_FUNCTION_NUMBER: u32 = 1 << 9;
    pub const R5_OUT_OF_RANGE: u32 = 1 << 8;
    pub const R5_ERROR_MASK: u32 = R5_ERROR | R5_FUNCTION_NUMBER | R5_OUT_OF_RANGE;
}

// =============================================================================
// sdhci.c sdhci_send_command 顺序（与 LicheeRV 完全一致）
// =============================================================================
//
// 1. sdhci_prepare_data(host, cmd)  // 有 data 时
//    - sdhci_set_block_info(host, data)  → 写 BLOCK_SIZE, BLOCK_COUNT
//    - DMA 路径：sdhci_config_dma (HOST_CTROL DMA_SEL), sdhci_set_sdma_addr(DMA_ADDRESS)
// 2. sdhci_writel(host, cmd->arg, SDHCI_ARGUMENT);
// 3. sdhci_set_transfer_mode(host, cmd);  → 写 TRANSFER_MODE (BLK_CNT_EN, MULTI, READ, DMA)
// 4. sdhci_writew(host, SDHCI_MAKE_CMD(cmd->opcode, flags), SDHCI_COMMAND);
//
// 等待：先 INT_RESPONSE(CMD 完成)，再 INT_DATA_END / INT_DMA_END(数据完成)。

/// 与 LicheeRV sdhci_send_command 一致的 CMD53 下发顺序说明（BSP 按此写寄存器）
/// 与 LicheeRV sdhci_prepare_data + sdhci_send_command 一致：HOST_CTRL(DMA_SEL) → DMA_ADDRESS → BLOCK_SIZE+BLOCK_COUNT → ARGUMENT → TRANSFER_MODE+COMMAND；wait CMD then DATA。
pub const SDHCI_CMD53_ORDER: &str = "HOST_CTRL(DMA_SEL) → DMA_ADDRESS → BLOCK_SIZE+BLOCK_COUNT → ARGUMENT → TRANSFER_MODE+COMMAND; wait CMD then DATA";
