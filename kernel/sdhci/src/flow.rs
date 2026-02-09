//! LicheeRV writesb/readsb 底层流程：与 Linux sdhci.c + sdio_ops.c 完全对齐
//!
//! 来源：
//! - LicheeRV-Nano-Build/linux_5.10/drivers/mmc/host/sdhci.c sdhci_send_command(1655 prepare_data, 1657 writel ARGUMENT, 1659 set_transfer_mode, 1712 writew COMMAND)
//! - sdio_ops.c mmc_io_rw_extended: data.blksz, data.blocks = blocks ? blocks : 1; arg 同 sdio_ops.rs
//! - SG2002 合并 BLOCK_SIZE+BLOCK_COUNT 为 BLK_SIZE_AND_CNT，顺序与 Linux 一致

// ========== Linux sdhci.h 寄存器偏移（SG2002 TRM 对应见 backend sdmmc_regs）==========
// SDHCI_DMA_ADDRESS    0x00  -> SDMA_SADDR
// SDHCI_BLOCK_SIZE     0x04  -> BLK_SIZE_AND_CNT[15:0]
// SDHCI_BLOCK_COUNT    0x06  -> BLK_SIZE_AND_CNT[31:16]
// SDHCI_ARGUMENT       0x08  -> ARGUMENT
// SDHCI_TRANSFER_MODE  0x0C  -> XFER_MODE_AND_CMD[15:0]
// SDHCI_COMMAND        0x0E  -> XFER_MODE_AND_CMD[31:16]
// SDHCI_HOST_CONTROL   0x28  -> HOST_CTRL1 (SDHCI_CTRL_DMA_MASK bits 4:3, 0=SDMA)
// SDHCI_INT_STATUS     0x30  -> NORM_AND_ERR_INT_STS

/// Linux sdhci_prepare_data (SDMA)：sdhci_config_dma → sdhci_set_sdma_addr → sdhci_set_block_info(BLOCK_SIZE, BLOCK_COUNT)
/// 即 HOST_CTRL(DMA_SEL=0) → SDMA_ADDRESS → BLOCK_SIZE + BLOCK_COUNT
#[allow(dead_code)]
pub const SDHCI_PREPARE_DATA_ORDER: &str = "HOST_CTRL -> SDMA_ADDRESS -> BLOCK_SIZE+BLOCK_COUNT";

/// Linux sdhci_send_command (有 data 时)：prepare_data 后 sdhci_writel(host, cmd->arg, SDHCI_ARGUMENT); sdhci_set_transfer_mode(host, cmd); sdhci_writew(host, SDHCI_MAKE_CMD(...), SDHCI_COMMAND);
#[allow(dead_code)]
pub const SDHCI_SEND_CMD_ORDER: &str = "ARGUMENT -> TRANSFER_MODE -> COMMAND";

/// SG2002 单次 CMD53 多块 DMA 整序（与 LicheeRV Linux 一致）：
/// 0. clear INT_STATUS；1. HOST_CTRL1 (DMA_SEL=0)；2. SDMA_SADDR；3. BLK_SIZE_AND_CNT (blocks<<16|blksz)；4. ARGUMENT；5. XFER_MODE_AND_CMD
/// 等待：INT_RESPONSE(CMD_CMPL) 再 INT_DATA_END/INT_DMA_END
#[allow(dead_code)]
pub const SDHCI_FULL_ORDER_SG2002: &str = "HOST_CTRL1 -> SDMA_SADDR -> BLK_SIZE_AND_CNT -> ARGUMENT -> XFER_MODE_AND_CMD; then wait CMD then DATA";

/// data.blocks 与 Linux sdio_ops.c 一致：blocks ? blocks : 1（字节模式时 host 仍写 1 块）
#[allow(dead_code)]
pub const SDHCI_DATA_BLOCKS_RULE: &str = "data.blocks = blocks ? blocks : 1";
