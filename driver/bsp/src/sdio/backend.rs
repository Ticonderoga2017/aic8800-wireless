//! AIC8800 SDIO 主机：基于 SG2002 SD1 的 CMD52/CMD53 实现
//!
//! 与 **LicheeRV-Nano-Build** 对齐：底层对应 Linux MMC 子系统的 CMD52/CMD53 与 aic8800 aicsdio.c 调用关系。
//!
//! ## LicheeRV 中的实现与调用链
//!
//! - **单字节**：`aicwf_sdio_readb` / `aicwf_sdio_writeb` → `sdio_readb` / `sdio_writeb` (Linux)
//!   → `mmc_io_rw_direct` (drivers/mmc/core/sdio_ops.c) → **CMD52** (SD_IO_RW_DIRECT)。
//! - **F1 wr_fifo 写**：`aicwf_sdio_send_pkt` → `sdio_writesb(func, wr_fifo_addr, buf, count)` (aicsdio.c)
//!   → `sdio_io_rw_ext_helper` → `mmc_io_rw_extended` → **CMD53** 字节模式、incr_addr=0（固定地址 FIFO）。
//! - **F1 rd_fifo 读**：`aicwf_sdio_recv_pkt` → `sdio_readsb(func, rd_fifo_addr, buf, size)` (aicsdio.c)
//!   → 同上，**CMD53** 字节模式读。
//!
//! ## CMD52/CMD53 参数与 Linux 一致
//!
//! - **CMD52**：arg = R/W(bit31) | fn<<28 | (raw) | addr<<9 | data_byte，与 mmc_io_rw_direct_host 一致。
//! - **CMD53**：arg 与 LicheeRV mmc_io_rw_extended 一致，经 sdio_ops::mmc_io_rw_extended_arg_byte/arg_block 构造；字节模式 blocks=0、
//!   blksz=N 时 Linux 为 data.blksz=N、data.blocks=1；本实现 BLK_SIZE_AND_CNT 设为 1 块 × N 字节以对齐。
//!
//! ## 与 LicheeRV 逻辑一致
//!
//! - F1 wr_fifo/rd_fifo、F2 消息口（含 0x207）均仅用 CMD53（与 sdio_writesb/sdio_readsb 一致），无 CMD52 块读写回退。
//! - CMD53 错误路径统一做 clear_int_status + reset_dat_line，避免 inhibit 未清除。

// =============================================================================
// 常量与寄存器（SG2002 TRM SDMMC）
// =============================================================================

/// SG2002 SD1 控制器物理基址（TRM memorymap_sg2002.table：0x04320000）
const SD1_PHYS_BASE: usize = 0x0432_0000;

/// RSTGEN 物理基址（SG2002 memorymap），SOFT_RSTN_0 偏移 0x000，Bit17 = SD1 复位（0=复位，1=释放）
const RSTGEN_PHYS: usize = 0x0300_3000;
const RSTGEN_SOFT_RSTN_0: usize = 0x000;
const RSTGEN_SD1_BIT: u32 = 1 << 17;

/// CLKGEN 物理基址（SG2002 TRM memorymap_sg2002.table：0x03002000）
const CLKGEN_PHYS: usize = 0x0300_2000;
/// clk_en_0 偏移；bit21=clk_axi4_sd1, bit22=clk_sd1, bit23=clk_100k_sd1（TRM div_crg_registers_description）
const CLKGEN_CLK_EN_0: usize = 0x000;
const CLKGEN_SD1_BITS: u32 = (1 << 21) | (1 << 22) | (1 << 23);

/// Active 域 PINMUX 基址（LicheeRV Nano U-Boot: 0x03001000）
const PINMUX_BASE: usize = 0x0300_1000;

/// SD1 SDIO pinmux 寄存器偏移（来自 U-Boot cvi_board_init.c）
mod sd1_pinmux {
    pub const D3: usize = 0x0D0;   // 0x030010D0
    pub const D2: usize = 0x0D4;   // 0x030010D4
    pub const D1: usize = 0x0D8;   // 0x030010D8
    pub const D0: usize = 0x0DC;   // 0x030010DC
    pub const CMD: usize = 0x0E0;  // 0x030010E0
    pub const CLK: usize = 0x0E4;  // 0x030010E4
    pub const WIFI_PWR: usize = 0x04C; // 0x0300104C - GPIOA_26 pinmux
}

/// 仅将 WiFi 电源引脚 (GPIOA_26) 的 pinmux 设为 GPIO 模式。
/// **必须在 aicbsp_power_on() 里、在首次驱动该 GPIO 之前调用**，否则引脚可能仍为默认功能，上电序列无效。
/// LicheeRV U-Boot: mmio_write_32(0x0300104C, 0x3)。
#[inline]
pub fn set_wifi_power_pinmux_to_gpio() {
    use axhal::mem::{pa, phys_to_virt};
    let pinmux_paddr = pa!(PINMUX_BASE);
    let pinmux_base = phys_to_virt(pinmux_paddr).as_usize();
    unsafe {
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::WIFI_PWR) as *mut u32, 0x3);
    }
    log::info!(target: "wireless::bsp::sdio", "set_wifi_power_pinmux_to_gpio: GPIOA_26 pinmux 0x{:08x}=0x3 (before power_on)", PINMUX_BASE + sd1_pinmux::WIFI_PWR);
}

/// 上电拉高后立即配置 SD1 数据/CMD/CLK pinmux（与 U-Boot cvi_board_init 顺序一致：high → pinmux → 无额外延时）。
/// 在稳定延时之前调用，使芯片在等待期间看到的 SDIO 引脚状态与 LicheeRV 一致。
#[inline]
pub fn set_sd1_sdio_pinmux_after_power() {
    use axhal::mem::{pa, phys_to_virt};
    let pinmux_paddr = pa!(PINMUX_BASE);
    let pinmux_base = phys_to_virt(pinmux_paddr).as_usize();
    unsafe {
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D3) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D2) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D1) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D0) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::CMD) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::CLK) as *mut u32, 0x0);
    }
    log::info!(target: "wireless::bsp::sdio", "set_sd1_sdio_pinmux_after_power: D3/D2/D1/D0/CMD/CLK=0 (align U-Boot: high then pinmux)");
}

/// 最小 SD1 主机初始化：释放复位、使能时钟、配置 pinmux（对照 LicheeRV Nano U-Boot）
fn sd1_host_init() {
    use axhal::mem::{pa, phys_to_virt};
    
    // 1. 释放 SD1 复位（Active 域 RSTGEN）：0=复位，1=释放；bootrom 未启动时先确认此处 bit17=1
    let rst_paddr = pa!(RSTGEN_PHYS);
    let rst_base = phys_to_virt(rst_paddr).as_usize();
    unsafe {
        let v = core::ptr::read_volatile((rst_base + RSTGEN_SOFT_RSTN_0) as *const u32);
        core::ptr::write_volatile((rst_base + RSTGEN_SOFT_RSTN_0) as *mut u32, v | RSTGEN_SD1_BIT);
        let readback = core::ptr::read_volatile((rst_base + RSTGEN_SOFT_RSTN_0) as *const u32);
        log::info!(target: "wireless::bsp::sdio", "sd1_host_init: RSTGEN 0x{:08x} SOFT_RSTN_0=0x{:08x} (bit17=1 => SD1 released)", RSTGEN_PHYS, readback);
    }

    // 2. 使能 SD1 时钟（CLKGEN）：bit21/22/23 = SD1 相关时钟；bootrom 未启动时确认此处已使能
    let clk_paddr = pa!(CLKGEN_PHYS);
    let clk_base = phys_to_virt(clk_paddr).as_usize();
    unsafe {
        let v = core::ptr::read_volatile((clk_base + CLKGEN_CLK_EN_0) as *const u32);
        core::ptr::write_volatile((clk_base + CLKGEN_CLK_EN_0) as *mut u32, v | CLKGEN_SD1_BITS);
        let readback = core::ptr::read_volatile((clk_base + CLKGEN_CLK_EN_0) as *const u32);
        log::info!(target: "wireless::bsp::sdio", "sd1_host_init: CLKGEN 0x{:08x} clk_en_0=0x{:08x} (SD1 bits 21/22/23)", CLKGEN_PHYS, readback);
    }

    // 3. 配置 SD1 SDIO pinmux（Active 域，来自 U-Boot cvi_board_init.c）
    //    值 0x0 = SD1 功能（默认可能是其他功能如 SPI）
    let pinmux_paddr = pa!(PINMUX_BASE);
    let pinmux_base = phys_to_virt(pinmux_paddr).as_usize();
    unsafe {
        // WiFi 电源 GPIO pinmux (GPIOA_26 = GPIO 模式 0x3)
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::WIFI_PWR) as *mut u32, 0x3);
        // SD1 数据/命令/时钟引脚 = SD1 功能 (0x0)
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D3) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D2) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D1) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::D0) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::CMD) as *mut u32, 0x0);
        core::ptr::write_volatile((pinmux_base + sd1_pinmux::CLK) as *mut u32, 0x0);
    }
    log::info!(target: "wireless::bsp::sdio", "sd1_host_init: configured SD1 pinmux (Active domain 0x{:08x}+D0-CLK=0, WiFi_PWR=GPIO)", PINMUX_BASE);
}

#[allow(dead_code)]
mod sdmmc_regs {
    pub const SDMA_SADDR: usize = 0x000;
    /// 块大小（11:0）与块数（31:16），CMD53 块模式使用
    pub const BLK_SIZE_AND_CNT: usize = 0x004;
    pub const ARGUMENT: usize = 0x008;
    pub const XFER_MODE_AND_CMD: usize = 0x00c;
    /// 与 LicheeRV SDHCI 一致：0x0C=TRANSFER_MODE(16bit)、0x0E=COMMAND(16bit)。多块 SDMA 时先写 TRANSFER_MODE 再写 COMMAND，使内部状态机正确进入完成节点。
    pub const TRANSFER_MODE: usize = 0x00c;
    pub const COMMAND: usize = 0x00e;
    pub const RESP31_0: usize = 0x010;
    pub const RESP63_32: usize = 0x014;
    /// 数据端口，CMD53 时读写 FIFO
    pub const BUF_DATA: usize = 0x020;
    /// 状态：CMD_INHIBIT(0)、CMD_INHIBIT_DAT(1)、BUF_WR_ENABLE(10)、BUF_RD_ENABLE(11)、CARD_INSERTED(16)
    pub const PRESENT_STS: usize = 0x024;
    /// 主机控制 1：LED(0)、DATA_WIDTH(1)、HI_SPEED(2)、DMA_SEL(4:3)、CARD_DET_TEST(6)、CARD_DET_SEL(7)
    pub const HOST_CTRL1: usize = 0x028;
    /// 与 LicheeRV SDHCI_POWER_CONTROL(0x29) 一致：写 SDHCI_POWER_ON|voltage 上电
    pub const POWER_CONTROL: usize = 0x029;
    /// 与 LicheeRV SDHCI_BLOCK_GAP_CONTROL(0x2A) 一致
    pub const BLOCK_GAP_CONTROL: usize = 0x02A;
    /// 与 LicheeRV SDHCI_WAKE_UP_CONTROL(0x2B) 一致
    pub const WAKE_UP_CONTROL: usize = 0x02B;
    /// 时钟与复位控制（TRM CLK_CTL_SWRST）：INT_CLK_EN(0)、INT_CLK_STABLE(1)、SD_CLK_EN(2)、FREQ_SEL(15:8)
    pub const CLK_CTL_SWRST: usize = 0x02c;
    /// 与 LicheeRV SDHCI_TIMEOUT_CONTROL(0x2E) 一致：quirk BROKEN_TIMEOUT_VAL 时用 0x0E
    pub const TIMEOUT_CONTROL: usize = 0x02E;
    /// 中断状态（与 LicheeRV SDHCI_INT_STATUS 语义一致，写回读出的值清除对应位）
    pub const NORM_AND_ERR_INT_STS: usize = 0x030;
    /// 中断状态使能
    pub const NORM_AND_ERR_INT_STS_EN: usize = 0x034;
    /// 中断信号使能
    pub const NORM_AND_ERR_INT_SIG_EN: usize = 0x038;
    /// LicheeRV P_VENDOR_SPECIFIC_AREA：读出的低 12 位为 vendor 区相对基址偏移
    pub const P_VENDOR_SPECIFIC_AREA: usize = 0x0E8;
    /// Cvitek vendor 区内偏移（与 sdhci-cv181x.h 一致）：MSHC_CTRL+0x40=PHY_TX_RX_DLY，+0x4C=PHY_CONFIG
    pub const VENDOR_PHY_TX_RX_DLY_OFF: usize = 0x40;
    pub const VENDOR_PHY_CONFIG_OFF: usize = 0x4C;
}

/// R5 响应错误位（与 LicheeRV include/linux/mmc/sdio.h 一致，检查 RESP 时用）。
/// R5 格式：resp[0] 低 32 位中 [23:16]= 状态（ERROR/OUT_OF_RANGE/INVALID_FUNCTION_NUMBER 等），[15:8]= 数据域。
/// 错误只看 (resp>>16)&R5_ERROR_MASK；resp=0x00002000 表示高 16 位为 0（无错误）、数据域=0x20，非卡端报错（主机超时时可能已收到 R5）。
const R5_ERROR_MASK: u32 = sdhci::sdhci::r5_error::R5_ERROR_MASK;
/// SDIO CCCR 0x05：Function Interrupt Pending（LicheeRV sdio_get_pending_irqs 用 CMD52 读此寄存器应答卡中断，使 CARD_INT 释放）
const SDIO_CCCR_INTX: u32 = 0x05;
/// 与 LicheeRV linux/drivers/mmc/host/sdhci.h 一致的中断位（NORM_AND_ERR_INT_STS 与 SDHCI_INT_STATUS 同构）
/// 与 LicheeRV sdhci.h 完全一致的 INT 位与掩码，用于 handle_sdhci_host_irq 与 Linux sdhci_irq/sdhci_data_irq 对齐
mod sdhci_int {
    pub const INT_RESPONSE: u32 = 0x0000_0001;
    pub const INT_DATA_END: u32 = 0x0000_0002;       // 数据阶段完成（成功）
    pub const INT_BLK_GAP: u32 = 0x0000_0004;
    pub const INT_DMA_END: u32 = 0x0000_0008;
    pub const INT_SPACE_AVAIL: u32 = 0x0000_0010;   // PIO 写空间可用
    pub const INT_DATA_AVAIL: u32 = 0x0000_0020;    // PIO 读数据可用
    pub const INT_RETUNE: u32 = 0x0000_1000;
    pub const INT_TIMEOUT: u32 = 0x0001_0000;       // 命令超时
    pub const INT_DATA_TIMEOUT: u32 = 0x0010_0000;
    pub const INT_DATA_CRC: u32 = 0x0020_0000;
    pub const INT_DATA_END_BIT: u32 = 0x0040_0000;
    pub const INT_BUS_POWER: u32 = 0x0080_0000;
    pub const INT_AUTO_CMD_ERR: u32 = 0x0100_0000;
    pub const INT_ADMA_ERROR: u32 = 0x0200_0000;   // sdhci_data_irq 中与 DATA_* 同组处理
    pub const INT_CRC: u32 = 0x0002_0000;
    pub const INT_END_BIT: u32 = 0x0004_0000;
    pub const INT_INDEX: u32 = 0x0008_0000;

    /// 与 sdhci.h SDHCI_INT_CMD_MASK 一致：清除/处理命令中断
    pub const INT_CMD_MASK: u32 = INT_RESPONSE | INT_TIMEOUT | INT_CRC | INT_END_BIT | INT_INDEX | INT_AUTO_CMD_ERR;
    /// 与 sdhci.h SDHCI_INT_DATA_MASK 一致：清除/处理数据中断（sdhci_irq 先 clear 再 sdhci_data_irq）
    pub const INT_DATA_MASK: u32 = INT_DATA_END | INT_DMA_END | INT_DATA_AVAIL | INT_SPACE_AVAIL
        | INT_DATA_TIMEOUT | INT_DATA_CRC | INT_DATA_END_BIT | INT_ADMA_ERROR | INT_BLK_GAP;
}

/// 与 LicheeRV sdhci_set_default_irqs 一致的默认 IER：INT_BUS_POWER | INT_DATA_END_BIT | INT_DATA_CRC | INT_DATA_TIMEOUT | INT_INDEX | INT_END_BIT | INT_CRC | INT_TIMEOUT | INT_DATA_END | INT_RESPONSE | INT_RETUNE
const DEFAULT_IER: u32 = sdhci_int::INT_BUS_POWER
    | sdhci_int::INT_DATA_END_BIT
    | sdhci_int::INT_DATA_CRC
    | sdhci_int::INT_DATA_TIMEOUT
    | sdhci_int::INT_INDEX
    | sdhci_int::INT_END_BIT
    | sdhci_int::INT_CRC
    | sdhci_int::INT_TIMEOUT
    | sdhci_int::INT_DATA_END
    | sdhci_int::INT_RESPONSE
    | sdhci_int::INT_RETUNE;

/// 轮询超时，防止死等（用于 wait_cmd_complete / wait_xfer_complete 等纯轮询）
const CMD_POLL_TIMEOUT_US: u32 = 100_000;
/// wait_not_inhibit 超时与步进（与 LicheeRV U-Boot sdhci.c 对齐：SDHCI_CMD_DEFAULT_TIMEOUT=100ms，每次 udelay(1000)=1ms）
const WAIT_INHIBIT_TIMEOUT_MS: u32 = 100;
/// CMD/DMA IRQ 等待超时（与 LicheeRV sdhci.c SDHCI_READ_STATUS_TIMEOUT=1000ms 对齐）；wait_cmd_complete_irq 的等待条件为「IRQ 中处理完 CMD 后 notify」，超时表示 1000ms 内未收到 notify
const WAIT_CMD_DMA_IRQ_TIMEOUT_MS: u64 = 1000;
const WAIT_INHIBIT_DELAY_MS: u32 = 1;
/// CMD53 单次最大字节数（SDIO 规范）
const CMD53_MAX_BYTES: usize = 512;

/// SDIO 命令索引（SD Physical Layer Spec）
const CMD_INDEX_0: u32 = 0;   // GO_IDLE_STATE
const CMD_INDEX_3: u32 = 3;   // SEND_RELATIVE_ADDR
const CMD_INDEX_5: u32 = 5;   // IO_SEND_OP_COND
const CMD_INDEX_7: u32 = 7;   // SELECT/DESELECT_CARD
const CMD_INDEX_52: u32 = 52; // IO_RW_Direct
const CMD_INDEX_53: u32 = 53; // IO_RW_Extended

/// 与 LicheeRV drivers/mmc/host/sdhci.h SDHCI_INT_CARD_INT 一致
const CARD_INT: u32 = 0x100;

/// 与 LicheeRV host->sdio_irq_pending 等价：CARD_INT 发生后置 true，process_sdio_pending_irqs 内清 false
static SDIO_IRQ_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// ---------- 与 LicheeRV sdhci_irq 等价的 CMD/DATA 完成路径：IRQ 里读 INT_STATUS、清除、再 complete ----------
use core::sync::atomic::Ordering;
use axtask::WaitQueue;

/// CMD53 多块 DMA 是否正在等待 CMD 完成（IRQ 里仅在此为 true 时对 CMD_CMPL 做 clear+notify）
static HOST_TRANSFER_CMD_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// CMD53 多块 DMA 是否正在等待 DMA 完成
static HOST_TRANSFER_DMA_PENDING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
/// DMA 流程与 LicheeRV sdhci_prepare_data + sdhci_data_irq 对齐：prepare 顺序为 DMA 上下文( base/total/bytes_xfered=0 ) → SDMA_SADDR → config_dma(HOST_CTRL) → set_transfer_irqs → set_block_info；IRQ 中 INT_DMA_END 按 dmastart+bytes_xfered 对齐下一 512KB 边界写回 SDMA_SA，仅在 INT_DATA_END 时 complete。
static HOST_DMA_BASE_PHYS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static HOST_DMA_TOTAL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static HOST_DMA_BYTES_XFERRED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static HOST_CMD_DONE_QUEUE: WaitQueue = WaitQueue::new();
static HOST_DMA_DONE_QUEUE: WaitQueue = WaitQueue::new();
/// IRQ 里写；wait 返回后读。0=成功，负=错误码
static HOST_CMD_RESULT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);
static HOST_DMA_RESULT: core::sync::atomic::AtomicI32 = core::sync::atomic::AtomicI32::new(0);

/// 与 LicheeRV sdhci_irq 等价：读 INT_STATUS → 先清除 CMD/DATA/BUS_POWER（DMA 等待时 DATA 延后按位清除）→ 处理 CMD/DATA/BUS_POWER/RETUNE/CARD_INT → 清除 CARD_INT/RETUNE；循环至无状态或 max_loops。
///
/// **CMD 完成与 LicheeRV 对齐**（LicheeRV 轮询：do { stat=read(); if (stat&INT_ERROR) break; } while ((stat&mask)!=mask)；成功则 (stat&(ERROR|mask))==mask 后 cmd_done+clear(mask)，否则 ret=-1 再 clear_all）：
/// - 清除顺序：**先** 写 INT_STATUS 清除 INT_CMD_MASK，**再** 根据本轮读到的 sts 写 result 并 notify（避免电平触发重复进中断，且用已读 sts 判结果）。
/// - Result 优先级（与 LicheeRV “break on INT_ERROR” 一致）：INT_TIMEOUT → -110；INT_CRC → -84；INT_END_BIT → -74；INT_RESPONSE → 0；其它 → -5。INT_RESPONSE 与 INT_TIMEOUT 同现时按超时返回。
/// - 写入与唤醒：先 HOST_CMD_RESULT.store(err)，再 HOST_TRANSFER_CMD_PENDING.store(false)，再 notify_one，保证 wait 端先读到 result 再被唤醒。
/// 由 irq.rs 的 sdio_irq_handler 在每次 SD1 中断时调用。
pub fn handle_sdhci_host_irq() {
    use axhal::mem::{pa, phys_to_virt};
    use sdhci_int::{INT_ADMA_ERROR, INT_CMD_MASK, INT_CRC, INT_DATA_CRC, INT_DATA_END, INT_DATA_END_BIT, INT_DATA_MASK, INT_DATA_TIMEOUT, INT_DMA_END, INT_END_BIT, INT_RESPONSE, INT_RETUNE, INT_TIMEOUT};
    const MAX_IRQ_LOOPS: u32 = 16; // 与 LicheeRV sdhci_irq max_loops 一致
    let base = phys_to_virt(pa!(SD1_PHYS_BASE)).as_usize();
    let mut max_loops = MAX_IRQ_LOOPS;
    unsafe {
        loop {
            let sts = core::ptr::read_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *const u32);
            if sts == 0 || sts == 0xffff_ffff {
                break;
            }
            log::trace!(target: "wireless::bsp::sdio", "handle_sdhci_host_irq: INT_STS=0x{:08x}", sts);

            // 与 LicheeRV 一致：先清除 CMD/DATA/BUS_POWER（有 DMA 等待时不在此处清 DATA，留到 DATA 分支按 DMA_END/DATA_END 分别清除）
            let mut clear_mask = sts & (INT_CMD_MASK | sdhci_int::INT_BUS_POWER);
            if !HOST_TRANSFER_DMA_PENDING.load(Ordering::SeqCst) {
                clear_mask |= sts & INT_DATA_MASK;
            }
            if clear_mask != 0 {
                core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, clear_mask);
            }

            // CMD 完成：与 LicheeRV 一致，错误优先于 INT_RESPONSE（同现时按超时/错误返回）；上面已清除 INT_CMD_MASK，此处仅用已读 sts 写 result 并 notify
            if (sts & INT_CMD_MASK) != 0 && HOST_TRANSFER_CMD_PENDING.load(Ordering::SeqCst) {
                let err = if (sts & INT_TIMEOUT) != 0 {
                    let r5 = core::ptr::read_volatile((base + sdmmc_regs::RESP31_0) as *const u32);
                    log::warn!(target: "wireless::bsp::sdio", "handle_sdhci_host_irq: CMD TIMEOUT INT_STS=0x{:08x} R5(RESP31_0)=0x{:08x} (0x2000=data byte 0x20, not R5 error)", sts, r5);
                    -110i32
                } else if (sts & INT_CRC) != 0 {
                    -84
                } else if (sts & INT_END_BIT) != 0 {
                    -74
                } else if (sts & INT_RESPONSE) != 0 {
                    0
                } else {
                    -5
                };
                HOST_CMD_RESULT.store(err, Ordering::SeqCst);
                HOST_TRANSFER_CMD_PENDING.store(false, Ordering::SeqCst);
                HOST_CMD_DONE_QUEUE.notify_one(true);
            }

            // DATA 完成：与 sdhci_data_irq 一致。先判 INT_DATA_END（成功；与 LicheeRV 一致，DATA_END 与 DATA_TIMEOUT 同现时以 DATA_END 为成功），再错误位，再 INT_DMA_END（只更新 SDMA_SA）。
            if (sts & INT_DATA_MASK) != 0 && HOST_TRANSFER_DMA_PENDING.load(Ordering::SeqCst) {
                let total = HOST_DMA_TOTAL.load(Ordering::SeqCst);
                let base_phys = HOST_DMA_BASE_PHYS.load(Ordering::SeqCst);

                if (sts & INT_DATA_END) != 0 {
                    HOST_DMA_RESULT.store(0, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                } else if (sts & INT_DATA_TIMEOUT) != 0 {
                    HOST_DMA_RESULT.store(-110i32, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                } else if (sts & INT_DATA_END_BIT) != 0 {
                    HOST_DMA_RESULT.store(-84, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                } else if (sts & INT_DATA_CRC) != 0 {
                    HOST_DMA_RESULT.store(-84, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                } else if (sts & INT_ADMA_ERROR) != 0 {
                    HOST_DMA_RESULT.store(-5, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                } else if (sts & INT_DMA_END) != 0 {
                    // 与 LicheeRV sdhci_data_irq INT_DMA_END 完全一致：dmastart+bytes_xfered 对齐到下一 512KB 边界，写回 SDMA_SADDR；不信任硬件返回的 DMA 地址（与 kernel 注释一致）
                    const SIZE: u32 = SDHCI_DEFAULT_BOUNDARY_SIZE as u32;
                    let bytes_so_far = HOST_DMA_BYTES_XFERRED.load(Ordering::SeqCst);
                    let dmanow = base_phys.wrapping_add(bytes_so_far as u32);
                    let next = (dmanow & !(SIZE - 1)).wrapping_add(SIZE);
                    let new_bytes = next.wrapping_sub(base_phys) as usize;
                    HOST_DMA_BYTES_XFERRED.store(new_bytes, Ordering::SeqCst);
                    if new_bytes < total {
                        core::ptr::write_volatile((base + sdmmc_regs::SDMA_SADDR) as *mut u32, next);
                    }
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, INT_DMA_END);
                } else {
                    HOST_DMA_RESULT.store(-5, Ordering::SeqCst);
                    HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
                    core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, sts & INT_DATA_MASK);
                    HOST_DMA_DONE_QUEUE.notify_one(true);
                }
            }

            // BUS_POWER：与 LicheeRV 一致仅记录
            if (sts & sdhci_int::INT_BUS_POWER) != 0 {
                log::trace!(target: "wireless::bsp::sdio", "handle_sdhci_host_irq: INT_BUS_POWER");
            }

            // INT_RETUNE：与 LicheeRV mmc_retune_needed 对齐（SDIO 无调谐，仅清除）
            // 在循环末统一清除 CARD_INT/RETUNE

            // CARD_INT：与 LicheeRV 一致仅当 (intmask & CARD_INT) && (ier & CARD_INT) 时 disable → signal → notify
            let ier = core::ptr::read_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS_EN) as *const u32);
            if (sts & CARD_INT) != 0 && (ier & CARD_INT) != 0 {
                let sts_en = core::ptr::read_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS_EN) as *const u32);
                let sig_en = core::ptr::read_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_SIG_EN) as *const u32);
                core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS_EN) as *mut u32, sts_en & !CARD_INT);
                core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_SIG_EN) as *mut u32, sig_en & !CARD_INT);
                SDIO_IRQ_PENDING.store(true, Ordering::SeqCst);
                super::irq::notify_sdio_irq_work();
            }

            // 与 LicheeRV 一致：清除已处理的 CARD_INT/RETUNE（首轮未写入 STATUS 的位）
            let clear_after = (sts & CARD_INT) | (sts & INT_RETUNE);
            if clear_after != 0 {
                core::ptr::write_volatile((base + sdmmc_regs::NORM_AND_ERR_INT_STS) as *mut u32, clear_after);
            }

            max_loops -= 1;
            if max_loops == 0 {
                break;
            }
        }
    }
}

// =============================================================================
// CMD0：GO_IDLE_STATE，复位卡到 Idle 状态
// =============================================================================

/// CMD0 无响应模式（广播命令）
const CMD0_XFER_MODE: u32 = 0
    | (0 << 16)  // RESP_TYPE_SEL = 0 (No Response)
    | (0 << 21)  // DATA_PRESENT_SEL = 0
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_0 << 24);

// =============================================================================
// CMD5：IO_SEND_OP_COND，识别 SDIO 卡并协商电压
// =============================================================================

/// CMD5 R4 响应模式（48 位，无 CRC 检查）
const CMD5_XFER_MODE: u32 = 0
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit)
    | (0 << 19)  // CMD_CRC_CHK_ENABLE = 0 (R4 无 CRC)
    | (0 << 20)  // CMD_IDX_CHK_ENABLE = 0
    | (0 << 21)  // DATA_PRESENT_SEL = 0
    | (0 << 22)  // CMD_TYPE = 0
    | (CMD_INDEX_5 << 24);

// =============================================================================
// CMD3：SEND_RELATIVE_ADDR，获取卡的 RCA
// =============================================================================

/// CMD3 R6 响应模式（48 位）
const CMD3_XFER_MODE: u32 = 0
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (0 << 21)  // DATA_PRESENT_SEL = 0
    | (0 << 22)  // CMD_TYPE = 0
    | (CMD_INDEX_3 << 24);

// =============================================================================
// CMD7：SELECT/DESELECT_CARD，选中卡进入 Transfer 状态
// =============================================================================

/// CMD7 R1b 响应模式（48 位 + busy）
const CMD7_XFER_MODE: u32 = 0
    | (3 << 16)  // RESP_TYPE_SEL = 3 (48-bit with busy / R1b)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (0 << 21)  // DATA_PRESENT_SEL = 0
    | (0 << 22)  // CMD_TYPE = 0
    | (CMD_INDEX_7 << 24);

// =============================================================================
// CMD52：IO_RW_Direct，单字节读写
// =============================================================================

/// XFER_MODE_AND_CMD 中 CMD52：R5 响应（48bit）、无数据、CMD52
/// 注意：位 15:0 是 Transfer Mode，对于无数据命令应全为 0
const CMD52_XFER_MODE: u32 = 0
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit / R5)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE (R5 有 CRC)
    | (1 << 20)  // CMD_IDX_CHK_ENABLE (R5 有 Index)
    | (0 << 21)  // DATA_PRESENT_SEL = 0 (无数据)
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_52 << 24);

// =============================================================================
// CMD53：IO_RW_Extended，块/字节扩展读写
// =============================================================================

/// CMD53 字节模式读：R5 响应、有数据、主机读、单块
/// Transfer Mode (位 15:0): bit 1 = 块计数使能 (1), bit 4 = 数据方向 (1=读)
/// Command (位 31:16): R5 响应、有数据
const CMD53_READ_XFER_MODE: u32 = 0
    | (1 << 1)   // BLOCK_COUNT_EN = 1
    | (1 << 4)   // DAT_XFER_DIR = 1 (Read, card→host)
    | (0 << 5)   // MULTI_BLK_SEL = 0 (单块)
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit / R5)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (1 << 21)  // DATA_PRESENT_SEL = 1 (有数据)
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_53 << 24);

/// CMD53 字节模式写：R5 响应、有数据、主机写、单块
const CMD53_WRITE_XFER_MODE: u32 = 0
    | (1 << 1)   // BLOCK_COUNT_EN = 1
    | (0 << 4)   // DAT_XFER_DIR = 0 (Write, host→card)
    | (0 << 5)   // MULTI_BLK_SEL = 0 (单块)
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit / R5)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (1 << 21)  // DATA_PRESENT_SEL = 1 (有数据)
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_53 << 24);

/// 与 LicheeRV sdhci.h 一致：Host SDMA buffer boundary，4K～512K 2 的幂；边界参数写入 BLOCK_SIZE 高 3 位。
/// SDHCI_DEFAULT_BOUNDARY_SIZE = 512*1024；SDHCI_DEFAULT_BOUNDARY_ARG = ilog2(512*1024)-12 = 7；SDHCI_MAKE_BLKSZ(7, 512) = 0x7200。
const SDHCI_DEFAULT_BOUNDARY_SIZE: usize = 512 * 1024;
const SDHCI_BLOCK_SIZE_WORD: u32 = ((7u32 & 0x7) << 12) | (512 & 0xFFF); // 0x7200

/// CMD53 块模式多块写：与 LicheeRV 一次 sdio_writesb(buf, 1536) 等价，一次 CMD53 传输多块（如 3×512），设备侧视为一条完整 IPC。
/// TRM SDMA 流程要求 XFER_MODE 置位 DMA_ENABLE(bit0)，否则控制器按非 DMA 等待 BUF_WRDY，CMD_CMPL 不置位。
const CMD53_WRITE_MULTI_XFER_MODE: u32 = 0
    | (1 << 0)   // DMA_ENABLE = 1（TRM: 1=DMA Data Transfer）
    | (1 << 1)   // BLOCK_COUNT_EN = 1
    | (0 << 4)   // DAT_XFER_DIR = 0 (Write)
    | (1 << 5)   // MULTI_BLK_SEL = 1 (多块)
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit / R5)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (1 << 21)  // DATA_PRESENT_SEL = 1 (有数据)
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_53 << 24);

/// CMD53 块模式多块读：与 sdio_readsb 一次读多块等价（LicheeRV aicwf_sdio_recv_pkt → sdio_readsb(..., size)）。
/// TRM SDMA 流程要求 XFER_MODE 置位 DMA_ENABLE(bit0)。
const CMD53_READ_MULTI_XFER_MODE: u32 = 0
    | (1 << 0)   // DMA_ENABLE = 1
    | (1 << 1)   // BLOCK_COUNT_EN = 1
    | (1 << 4)   // DAT_XFER_DIR = 1 (Read)
    | (1 << 5)   // MULTI_BLK_SEL = 1 (多块)
    | (2 << 16)  // RESP_TYPE_SEL = 2 (48-bit / R5)
    | (1 << 19)  // CMD_CRC_CHK_ENABLE
    | (1 << 20)  // CMD_IDX_CHK_ENABLE
    | (1 << 21)  // DATA_PRESENT_SEL = 1 (有数据)
    | (0 << 22)  // CMD_TYPE = 0 (Normal)
    | (CMD_INDEX_53 << 24);

// =============================================================================
// AIC8800 SDIO 主机（唯一实现）
// =============================================================================

/// 内部卡时钟频率（Hz），用于 set_clock 分频计算。TRM: F_SD_CLK = F_INT_CARD_CLK / (2*divisor)。与 LicheeRV DTS src-frequency = 375000000、max-frequency = 25000000 一致。
const INT_CARD_CLK_HZ: u32 = 375_000_000;
/// 与 LicheeRV set_ios(clock) 一致：枚举阶段 400kHz，数据阶段 25MHz。
pub const FREQ_SEL_DATA_RATE: u8 = 2;

/// AIC8800 使用的 SDIO 主机：基于 SG2002 SD1，通过 CMD52/CMD53 访问 AIC8800 卡。
///
/// - **CMD52**：单字节读写（IO_RW_Direct），对应 Linux sdio_readb/sdio_writeb。
/// - **CMD53**：块/字节扩展读写（IO_RW_Extended），对应 sdio_readsb/sdio_writesb。
///
/// 使用前需保证 SD 主机已初始化（时钟、上电、卡已识别）。
#[derive(Debug)]
pub struct Aic8800SdioHost {
    /// SDMMC 控制器虚拟基址（由 phys_base 经 phys_to_virt 得到）
    base_vaddr: usize,
}

impl Aic8800SdioHost {
    /// 从物理基址构造主机。
    ///
    /// # 参数
    /// - `phys_base`: SDMMC 控制器物理基址，如 `0x0432_0000`（SG2002 SD1）。
    pub fn new(phys_base: usize) -> Self {
        use axhal::mem::{pa, phys_to_virt};
        let paddr = pa!(phys_base);
        let vaddr = phys_to_virt(paddr);
        Self {
            base_vaddr: vaddr.as_usize(),
        }
    }

    /// 使用 SG2002 SD1 默认基址（0x04320000）构造；内部会调用 SD1 主机最小初始化（RSTGEN+CLKGEN），并打开控制器接口时钟（CLK_CTL）。
    ///
    /// **注意**：此函数只初始化 SD 控制器，不进行卡枚举。要与卡通信需先调用 `sdio_card_init()`。
    pub fn new_sd1() -> Self {
        sd1_host_init();
        let host = Self::new(SD1_PHYS_BASE);
        host.enable_sd_interface_clock();
        host
    }

    /// 使用 SG2002 SD1 默认基址构造，并完成 SDIO 卡枚举（CMD0→CMD5→CMD3→CMD7）。
    ///
    /// 成功返回 `(host, rca)`，失败返回错误码。枚举完成后卡处于 Transfer 状态，可用 CMD52/CMD53 通信。
    pub fn new_sd1_with_card_init() -> Result<(Self, u16), i32> {
        sd1_host_init();
        let host = Self::new(SD1_PHYS_BASE);
        host.enable_sd_interface_clock();
        let rca = host.sdio_card_init()?;
        Ok((host, rca))
    }

    /// 使能 SDMMC 控制器接口时钟（与 LicheeRV sdhci_set_ios → set_clock 顺序一致：reset → HOST_CTRL → IER → set_clock(init)）。
    fn enable_sd_interface_clock(&self) {
        // 1. 软复位整个控制器（与 LicheeRV sdhci_cvi_reset_helper + SDHCI_RESET_ALL 等价）
        self.reset_all();

        // 2. 与 LicheeRV sdhci_set_ios/sdhci_set_power 对齐：POWER_CONTROL 上电 3.3V；BLOCK_GAP/WAKE_UP 清 0；TIMEOUT 默认 0x0E（QUIRK_BROKEN_TIMEOUT_VAL）
        const SDHCI_POWER_ON: u8 = 0x01;
        const SDHCI_POWER_330: u8 = 0x0E;
        unsafe {
            self.write_reg_8(sdmmc_regs::POWER_CONTROL, SDHCI_POWER_ON | SDHCI_POWER_330);
            self.write_reg_8(sdmmc_regs::BLOCK_GAP_CONTROL, 0);
            self.write_reg_8(sdmmc_regs::WAKE_UP_CONTROL, 0);
            self.write_reg_8(sdmmc_regs::TIMEOUT_CONTROL, 0x0E);
        }
        log::debug!(target: "wireless::bsp::sdio", "POWER=0x{:02x} BLOCK_GAP=0 WAKE_UP=0 TIMEOUT=0x0E (align LicheeRV)", SDHCI_POWER_ON | SDHCI_POWER_330);

        // 3. 配置 HOST_CTRL1：强制卡检测（SDIO 模组无 CD 引脚）
        const CARD_DET_TEST: u32 = 1 << 6;
        const CARD_DET_SEL: u32 = 1 << 7;
        let host_ctrl = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
        unsafe { self.write_reg(sdmmc_regs::HOST_CTRL1, host_ctrl | CARD_DET_TEST | CARD_DET_SEL) };
        log::debug!(target: "wireless::bsp::sdio", "HOST_CTRL1: 0x{:08x} -> 0x{:08x} (force card detect)", host_ctrl, host_ctrl | CARD_DET_TEST | CARD_DET_SEL);

        // 4. 与 LicheeRV cv181x sdio reset 对齐：VENDOR MSHC_CTRL bit0|bit1|bit16；PHY 为 DS/HS（0x240=0x1000100，0x24c bit0=1）
        {
            let vendor_area = unsafe { self.read_reg(sdmmc_regs::P_VENDOR_SPECIFIC_AREA) };
            let vendor_base = (vendor_area & 0xFFF) as usize;
            let mshc_ctrl_offset = vendor_base;
            const MSHC_BIT0: u32 = 1 << 0;  // EMMC_FUNC_EN
            const MSHC_BIT1: u32 = 1 << 1;  // DS/HS setting
            const MSHC_BIT16: u32 = 1 << 16; // for sd1
            let ctrl = unsafe { self.read_reg(mshc_ctrl_offset) };
            unsafe { self.write_reg(mshc_ctrl_offset, ctrl | MSHC_BIT0 | MSHC_BIT1 | MSHC_BIT16) };
            let phy_dly_offset = vendor_base + sdmmc_regs::VENDOR_PHY_TX_RX_DLY_OFF;
            let phy_config_offset = vendor_base + sdmmc_regs::VENDOR_PHY_CONFIG_OFF;
            unsafe {
                self.write_reg(phy_dly_offset, 0x1000100); // LicheeRV DS/HS: reg_0x240[25:24]=1 [22:16]=0 [9:8]=1 [6:0]=0
                let cfg = self.read_reg(phy_config_offset);
                self.write_reg(phy_config_offset, cfg | 1); // reg_0x24c[0]=1
            }
            log::info!(target: "wireless::bsp::sdio", "MSHC_CTRL(base+0x{:03x})=0x{:08x} PHY_DLY=0x1000100 PHY_CFG bit0=1 (SDIO SD1 DS/HS, align LicheeRV)", mshc_ctrl_offset, ctrl | MSHC_BIT0 | MSHC_BIT1 | MSHC_BIT16);
        }

        // 5. 与 LicheeRV sdhci_set_default_irqs 一致：默认 IER 成对写入
        unsafe {
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN, DEFAULT_IER);
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_SIG_EN, DEFAULT_IER);
        }
        log::debug!(target: "wireless::bsp::sdio", "INT_STS_EN / INT_SIG_EN: DEFAULT_IER=0x{:08x}", DEFAULT_IER);

        // 6. 与 LicheeRV set_ios(clock) → set_clock 一致：枚举阶段 400kHz
        self.set_clock(400_000);
    }

    /// 设置 SD 总线时钟（与 LicheeRV host->ops->set_clock(host, ios->clock) 对齐）。
    /// TRM: F_SD_CLK = F_INT_CARD_CLK / (2*divisor)，FREQ_SEL(15:8)=divisor。
    /// - `clock_hz == 0`：关闭卡时钟（SD_CLK_EN=0），内部时钟保持。
    /// - `clock_hz > 0`：计算 divisor，设 FREQ_SEL，开启 INT_CLK → 等 INT_CLK_STABLE → 开启 SD_CLK。
    pub fn set_clock(&self, clock_hz: u32) {
        const INT_CLK_EN: u32 = 1 << 0;
        const INT_CLK_STABLE: u32 = 1 << 1;
        const SD_CLK_EN: u32 = 1 << 2;
        const FREQ_SEL_MASK: u32 = 0xFF00;

        if clock_hz == 0 {
            self.set_host_high_speed(false);
            let ctl = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, ctl & !SD_CLK_EN) };
            log::debug!(target: "wireless::bsp::sdio", "set_clock(0): SD_CLK_EN off");
            return;
        }

        let divisor = (INT_CARD_CLK_HZ + 2 * clock_hz - 1) / (2 * clock_hz);
        let divisor = divisor.clamp(1, 255);
        let freq_sel = (divisor as u32) << 8;

        let mut ctl = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
        ctl = (ctl & !FREQ_SEL_MASK) | freq_sel | INT_CLK_EN;
        unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, ctl) };
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        for _ in 0..CMD_POLL_TIMEOUT_US {
            let st = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            if (st & INT_CLK_STABLE) != 0 {
                unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, st | SD_CLK_EN) };
                self.set_host_high_speed(clock_hz >= 20_000_000);
                let actual_hz = INT_CARD_CLK_HZ / (2 * divisor);
                log::info!(target: "wireless::bsp::sdio", "set_clock: requested {} Hz -> divisor {} actual ~{} Hz (FREQ_SEL=0x{:02x})", clock_hz, divisor, actual_hz, divisor);
                return;
            }
            core::hint::spin_loop();
        }
        let st = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
        unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, st | SD_CLK_EN) };
        self.set_host_high_speed(clock_hz >= 20_000_000);
        log::warn!(target: "wireless::bsp::sdio", "set_clock: INT_CLK_STABLE did not assert; SD_CLK_EN set anyway, divisor={}", divisor);
    }

    /// 主机侧高速使能（与 LicheeRV sdhci_set_ios 中 ios->timing==MMC_TIMING_SD_HS 时置 SDHCI_CTRL_HISPD 一致）。
    fn set_host_high_speed(&self, enable: bool) {
        const SDHCI_CTRL_HISPD: u32 = 1 << 2;
        let ctrl = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
        let new_ctrl = if enable { ctrl | SDHCI_CTRL_HISPD } else { ctrl & !SDHCI_CTRL_HISPD };
        if new_ctrl != ctrl {
            unsafe { self.write_reg(sdmmc_regs::HOST_CTRL1, new_ctrl) };
            log::debug!(target: "wireless::bsp::sdio", "set_host_high_speed: {} (HOST_CTRL1 HISPD)", enable);
        }
    }

    /// 软复位整个控制器（与 LicheeRV sdhci_cvi_reset_helper + sdhci_reset(host, SDHCI_RESET_ALL) 对齐）。
    /// TRM: CLK_CTL_SWRST bit24=SW_RST_ALL（等价 SDHCI 0x2F RESET_ALL）。流程：关 INT_STS_EN/INT_SIG_EN → 写 SW_RST_ALL → 等 bit 自清 → 恢复 DEFAULT_IER。
    fn reset_all(&self) {
        const SW_RST_ALL: u32 = 1 << 24;
        unsafe {
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN, 0);
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_SIG_EN, 0);
        }
        unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, SW_RST_ALL) };
        for _ in 0..100000 {
            let v = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            if (v & SW_RST_ALL) == 0 {
                log::debug!(target: "wireless::bsp::sdio", "reset_all: controller reset complete");
                unsafe {
                    self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN, DEFAULT_IER);
                    self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_SIG_EN, DEFAULT_IER);
                }
                return;
            }
            core::hint::spin_loop();
        }
        log::warn!(target: "wireless::bsp::sdio", "reset_all: SW_RST_ALL did not auto-clear");
    }

    // =========================================================================
    // SDIO 卡枚举流程：CMD0 → CMD5 → CMD3 → CMD7
    // =========================================================================

    /// **SDIO 卡枚举**：发送 CMD0→CMD5→CMD3→CMD7，将卡从 Idle 状态带入 Transfer 状态。
    ///
    /// 成功后返回卡的 RCA (Relative Card Address)，之后即可使用 CMD52/CMD53 访问卡。
    pub fn sdio_card_init(&self) -> Result<u16, i32> {
        log::info!(target: "wireless::bsp::sdio", "sdio_card_init: starting SDIO card enumeration...");

        // 诊断：打印初始状态
        let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        let clk_ctl = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
        log::debug!(target: "wireless::bsp::sdio", "sdio_card_init: initial PRESENT_STS=0x{:08x}, CLK_CTL=0x{:08x}", present, clk_ctl);

        // 0. 发送至少 74 个时钟周期让卡上电稳定（SD 规范要求）
        //    通过延时实现（时钟已使能）
        for _ in 0..100000 {
            core::hint::spin_loop();
        }

        // 1. CMD0: GO_IDLE_STATE - 复位卡到 Idle 状态
        self.cmd0_go_idle()?;
        // 给卡一些时间复位
        for _ in 0..50000 {
            core::hint::spin_loop();
        }
        log::debug!(target: "wireless::bsp::sdio", "sdio_card_init: CMD0 sent (card reset to Idle)");

        // 2. CMD5: IO_SEND_OP_COND - 识别 SDIO 卡
        //    第一次 arg=0，获取卡支持的 OCR
        let ocr = self.cmd5_io_send_op_cond(0)?;
        log::debug!(target: "wireless::bsp::sdio", "sdio_card_init: CMD5(0) -> OCR=0x{:08x}", ocr);

        if ocr == 0 {
            log::error!(target: "wireless::bsp::sdio", "sdio_card_init: CMD5 returned OCR=0, no SDIO card?");
            return Err(-19); // -ENODEV
        }

        //    第二次 CMD5 带电压选择位，等待卡 Ready
        let mut ready = false;
        for retry in 0..100 {
            // 设置支持的电压范围 (3.2-3.4V)，bit 20-21
            let arg = ocr & 0x00FF_FF00; // 保留电压位
            let resp = self.cmd5_io_send_op_cond(arg)?;
            log::trace!(target: "wireless::bsp::sdio", "sdio_card_init: CMD5({:08x}) retry {} -> 0x{:08x}", arg, retry, resp);
            // R4 响应 bit31 = C (Card ready)
            if (resp & 0x8000_0000) != 0 {
                ready = true;
                log::debug!(target: "wireless::bsp::sdio", "sdio_card_init: card ready after {} retries, OCR=0x{:08x}", retry, resp);
                break;
            }
            // 短暂延时后重试
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
        if !ready {
            log::error!(target: "wireless::bsp::sdio", "sdio_card_init: card not ready after CMD5 retries");
            return Err(-110); // -ETIMEDOUT
        }

        // 3. CMD3: SEND_RELATIVE_ADDR - 获取 RCA
        let rca = self.cmd3_send_relative_addr()?;
        log::info!(target: "wireless::bsp::sdio", "sdio_card_init: CMD3 -> RCA=0x{:04x}", rca);

        // 4. CMD7: SELECT_CARD - 选中卡，进入 Transfer 状态
        self.cmd7_select_card(rca)?;
        log::info!(target: "wireless::bsp::sdio", "sdio_card_init: CMD7 sent, card selected (RCA=0x{:04x})", rca);

        // 5. 与 Linux 一致：首次 CMD52（读 CCCR）须在 1-bit 下进行；4-bit 在 sdio_enable_4bit_bus 时再开（见 flow 中 enable_4bit_bus 调用）
        //    Linux 顺序：mmc_attach_sdio → sdio_read_cccr(1-bit) → ... → sdio_enable_4bit_bus() → mmc_set_bus_width(4)
        log::info!(target: "wireless::bsp::sdio", "sdio_card_init: SDIO card enumeration complete (1-bit), card in Transfer state");
        Ok(rca)
    }

    /// 设置主机总线位宽（与 LicheeRV sdhci_set_bus_width 对齐：SDHCI_HOST_CONTROL bit1 = SDHCI_CTRL_4BITBUS，即 HOST_CTRL1 bit1）。
    pub fn set_bus_width(&self, width_4: bool) {
        const DAT_XFER_WIDTH_4BIT: u32 = 1 << 1; // = SDHCI_CTRL_4BITBUS
        let host_ctrl = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
        let new_ctrl = if width_4 {
            host_ctrl | DAT_XFER_WIDTH_4BIT
        } else {
            host_ctrl & !DAT_XFER_WIDTH_4BIT
        };
        unsafe { self.write_reg(sdmmc_regs::HOST_CTRL1, new_ctrl) };
        log::debug!(target: "wireless::bsp::sdio", "set_bus_width: {} (HOST_CTRL1 0x{:08x} -> 0x{:08x})", if width_4 { "4-bit" } else { "1-bit" }, host_ctrl, new_ctrl);
    }

    /// 使能 SDIO 卡高速模式（与 LicheeRV sdio_enable_hs → mmc_sdio_switch_hs 一致：CCCR 0x13 若支持 SHS 则置 EHS）。
    /// 在切到 25MHz 前调用，与 LicheeRV 快模式顺序一致：enable_hs → set_timing(SD_HS) → set_clock(max) → enable_4bit_bus。
    pub fn enable_sdio_high_speed(&self) -> Result<(), i32> {
        const SDIO_CCCR_SPEED: u32 = 0x13;
        const SDIO_SPEED_SHS: u8 = 0x01; // Supports High-Speed
        const SDIO_SPEED_EHS: u8 = 0x02; // Enable High-Speed (SDR25)
        let speed = self.read_byte(SDIO_CCCR_SPEED)?;
        if (speed & SDIO_SPEED_SHS) == 0 {
            return Ok(());
        }
        self.write_byte(SDIO_CCCR_SPEED, speed | SDIO_SPEED_EHS)?;
        log::info!(target: "wireless::bsp::sdio", "enable_sdio_high_speed: CCCR 0x13 EHS set (align LicheeRV mmc_sdio_switch_hs)");
        Ok(())
    }

    /// 启用 4-bit 总线（与 Linux sdio_enable_4bit_bus → sdio_enable_wide + mmc_set_bus_width(4) 一致）。
    /// 先写卡 CCCR_IF(0x07)，再调 set_bus_width(4) 与 LicheeRV 顺序一致。
    pub fn enable_4bit_bus(&self) {
        const SDIO_CCCR_IF: u32 = 0x07;
        const SDIO_BUS_WIDTH_MASK: u8 = 0x03;
        const SDIO_BUS_WIDTH_4BIT: u8 = 0x02;
        if let Ok(ctrl) = self.read_byte(SDIO_CCCR_IF) {
            let ctrl = (ctrl & !SDIO_BUS_WIDTH_MASK) | SDIO_BUS_WIDTH_4BIT;
            if let Err(e) = self.write_byte(SDIO_CCCR_IF, ctrl) {
                log::warn!(target: "wireless::bsp::sdio", "enable_4bit_bus: write CCCR_IF(0x07) failed {}", e);
            }
        } else {
            log::warn!(target: "wireless::bsp::sdio", "enable_4bit_bus: read CCCR_IF(0x07) failed");
        }
        self.set_bus_width(true);
        log::info!(target: "wireless::bsp::sdio", "enable_4bit_bus: CCCR_IF 4-bit + set_bus_width(4) (align LicheeRV sdio_enable_4bit_bus)");
    }

    /// CMD0: GO_IDLE_STATE - 复位卡到 Idle 状态（无响应）
    fn cmd0_go_idle(&self) -> Result<(), i32> {
        // 发送 CMD0 前先软复位命令线，确保控制器状态干净
        self.reset_cmd_line();
        self.wait_not_inhibit()?;
        self.clear_int_status(); // 与 U-Boot 一致：发命令前清掉上次/上电残留的中断
        // 诊断：发送前状态
        let pre_present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        let pre_int = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::debug!(target: "wireless::bsp::sdio", "CMD0 pre: PRESENT_STS=0x{:08x}, INT_STS=0x{:08x}", pre_present, pre_int);
        
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, 0);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD0_XFER_MODE);
        }
        
        // 短暂延时让命令开始发送
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        
        // 诊断：发送后立即状态
        let post_present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        let post_int = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::debug!(target: "wireless::bsp::sdio", "CMD0 post: PRESENT_STS=0x{:08x}, INT_STS=0x{:08x}", post_present, post_int);
        
        // CMD0 无响应，但仍需等待控制器完成命令发送
        // 对于无响应命令，CMD_CMPL 会在命令发送完成后置位
        self.wait_cmd_complete_no_resp()?;
        Ok(())
    }

    /// 软复位命令线（CLK_CTL_SWRST bit 25）
    fn reset_cmd_line(&self) {
        const SW_RST_CMD: u32 = 1 << 25;
        let val = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
        unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, val | SW_RST_CMD) };
        // 等待复位完成（bit 自动清除）
        for _ in 0..10000 {
            let v = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            if (v & SW_RST_CMD) == 0 {
                return;
            }
            core::hint::spin_loop();
        }
        log::warn!(target: "wireless::bsp::sdio", "reset_cmd_line: SW_RST_CMD did not auto-clear");
    }

    /// 软复位数据线（CLK_CTL_SWRST bit 26，TRM SW_RST_DAT）
    ///
    /// 当 CMD_INHIBIT_DAT 一直为 1（DAT 线忙/卡 R1b 未释放等）时，可复位 DAT 线清除内部状态，
    /// 使 PRESENT_STS[CMD_INHIBIT_DAT] 恢复为 0。卡仍处于 Transfer 状态，无需重新枚举。
    pub fn reset_dat_line(&self) {
        const SW_RST_DAT: u32 = 1 << 26;
        let val = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
        unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, val | SW_RST_DAT) };
        for _ in 0..10000 {
            let v = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            if (v & SW_RST_DAT) == 0 {
                log::debug!(target: "wireless::bsp::sdio", "reset_dat_line: SW_RST_DAT complete");
                return;
            }
            core::hint::spin_loop();
        }
        log::warn!(target: "wireless::bsp::sdio", "reset_dat_line: SW_RST_DAT did not auto-clear");
    }

    /// 等待无响应命令完成（CMD0 等）
    fn wait_cmd_complete_no_resp(&self) -> Result<(), i32> {
        // 对于无响应命令，有些控制器不会设置 CMD_CMPL，只需等待 CMD_INHIBIT 清除
        // 同时检查错误位
        for _ in 0..CMD_POLL_TIMEOUT_US {
            let sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            // TRM RWC：写读出的值清除对应位，必须写完整 32 位否则 PRESENT_STS 可能不更新
            if sts != 0 {
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts) };
            }
            // 检查 CMD_INHIBIT 是否清除
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            if (present & 1) == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_resp: timeout PRESENT_STS=0x{:08x}", present);
        Err(-110)
    }

    /// CMD5: IO_SEND_OP_COND - 识别 SDIO 卡并协商电压
    ///
    /// 返回 R4 响应的 OCR 值：
    /// - bit 31: C (Card ready)
    /// - bit 30-28: Number of I/O functions
    /// - bit 27: Memory Present
    /// - bit 23-0: OCR (Operating Conditions Register)
    fn cmd5_io_send_op_cond(&self, arg: u32) -> Result<u32, i32> {
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在发令前清 INT_STATUS，仅依赖轮询时按需清除
        let pre_int = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::debug!(target: "wireless::bsp::sdio", "CMD5({:08x}) pre: INT_STS=0x{:08x}", arg, pre_int);
        
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD5_XFER_MODE);
        }
        
        // 等待命令完成（CMD5 有 R4 响应）
        let wait_result = self.wait_cmd_complete_no_crc();
        
        let post_int = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let post_present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::debug!(target: "wireless::bsp::sdio", "CMD5 post: INT_STS=0x{:08x}, PRESENT_STS=0x{:08x}, wait_result={:?}", post_int, post_present, wait_result);
        
        wait_result?;
        
        // R4 响应在 RESP31_0
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        let resp_hi = unsafe { self.read_reg(sdmmc_regs::RESP63_32) };
        log::debug!(target: "wireless::bsp::sdio", "CMD5 response: RESP31_0=0x{:08x}, RESP63_32=0x{:08x}", resp, resp_hi);
        Ok(resp)
    }

    /// CMD3: SEND_RELATIVE_ADDR - SDIO 卡返回 RCA
    ///
    /// 与 SD 卡不同，SDIO 卡自己生成 RCA 并通过 R6 响应返回。
    fn cmd3_send_relative_addr(&self) -> Result<u16, i32> {
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在发令前清 INT_STATUS
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, 0);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD3_XFER_MODE);
        }
        self.wait_cmd_complete()?;
        // R6 响应：[31:16] = RCA, [15:0] = card status
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        let rca = ((resp >> 16) & 0xFFFF) as u16;
        let status = (resp & 0xFFFF) as u16;
        log::trace!(target: "wireless::bsp::sdio", "CMD3 resp=0x{:08x}, RCA=0x{:04x}, status=0x{:04x}", resp, rca, status);
        Ok(rca)
    }

    /// CMD7: SELECT/DESELECT_CARD - 选中卡进入 Transfer 状态
    ///
    /// R1b 响应：卡会保持 DAT0 为忙直到内部就绪，SDHCI 的 CMD_INHIBIT_DAT 会保持置位。
    /// 必须调用 wait_not_inhibit() 等待卡释放 DAT0，否则后续 CMD53 会因 wait_not_inhibit 超时失败
    ///（对照 LicheeRV：Linux MMC 栈在 CMD 完成后会等待 DAT 线空闲）。
    fn cmd7_select_card(&self, rca: u16) -> Result<(), i32> {
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在发令前清 INT_STATUS
        let arg = (rca as u32) << 16;
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD7_XFER_MODE);
        }
        // R1b 响应：先等命令完成
        self.wait_cmd_complete()?;
        // 必须等待 DAT0 不再 busy（PRESENT_STS CMD_INHIBIT_DAT 清除），否则后续 CMD53 会超时
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        self.wait_not_inhibit()?;
        Ok(())
    }

    /// 等待命令完成（用于 R4 等无 CRC 检查的响应）
    ///
    /// SDHCI 中断状态位定义（对照 Linux sdhci.h）：
    /// - bit 0: CMD_CMPL (SDHCI_INT_RESPONSE)
    /// - bit 16: CMD_TIMEOUT (SDHCI_INT_TIMEOUT) 
    /// - bit 17: CMD_CRC_ERR (SDHCI_INT_CRC) - R4 无 CRC，忽略
    /// - bit 18: CMD_END_BIT_ERR (SDHCI_INT_END_BIT)
    /// - bit 19: CMD_INDEX_ERR (SDHCI_INT_INDEX)
    fn wait_cmd_complete_no_crc(&self) -> Result<(), i32> {
        const INT_CMD_CMPL: u32 = 1 << 0;
        const INT_CMD_TIMEOUT: u32 = 1 << 16;
        const INT_CMD_CRC: u32 = 1 << 17;    // R4 忽略
        const INT_CMD_END_BIT: u32 = 1 << 18;
        const INT_CMD_INDEX: u32 = 1 << 19;
        // 命令错误掩码（不含 CRC，因为 R4 无 CRC）
        const INT_CMD_ERR_MASK: u32 = INT_CMD_TIMEOUT | INT_CMD_END_BIT | INT_CMD_INDEX;

        for i in 0..CMD_POLL_TIMEOUT_US {
            let sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            
            // 检查命令错误（不含 CRC）
            if (sts & INT_CMD_ERR_MASK) != 0 {
                // 清除所有状态
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts) };
                if (sts & INT_CMD_TIMEOUT) != 0 {
                    log::warn!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_crc: CMD_TIMEOUT (no card?), INT_STS=0x{:08x}", sts);
                    return Err(-110);  // ETIMEDOUT
                }
                if (sts & INT_CMD_END_BIT) != 0 {
                    log::warn!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_crc: CMD_END_BIT_ERR (bad response format), INT_STS=0x{:08x}", sts);
                    return Err(-74);  // EBADMSG
                }
                if (sts & INT_CMD_INDEX) != 0 {
                    log::warn!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_crc: CMD_INDEX_ERR, INT_STS=0x{:08x}", sts);
                    return Err(-5);   // EIO
                }
            }
            
            // 检查命令完成
            if (sts & INT_CMD_CMPL) != 0 {
                // 清除状态
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts) };
                log::trace!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_crc: CMD_CMPL after {} iterations, INT_STS=0x{:08x}", i, sts);
                return Ok(());
            }
            core::hint::spin_loop();
        }
        let final_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_cmd_complete_no_crc: poll timeout, final INT_STS=0x{:08x}", final_sts);
        Err(-110)
    }

    /// 读 PRESENT_STS 用于诊断：可发命令时 bit0/bit1 为 0；未初始化时常为 0 或 0xFFFFFFFF。
    pub fn read_present_sts(&self) -> u32 {
        unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) }
    }

    /// 应用主机接口配置（对应 Linux host->ops->set_ios：clock + bus_width）。
    /// - `four_bit`: 调用 set_bus_width(four_bit)
    /// - `freq_sel`: 若为 Some(v)，设置 CLK_CTL FREQ_SEL(15:8)=v；若需按 Hz 设时钟请用 set_clock(clock_hz)
    pub fn set_ios_raw(&self, four_bit: bool, freq_sel: Option<u8>) -> Result<(), i32> {
        self.set_bus_width(four_bit);
        if let Some(v) = freq_sel {
            const FREQ_SEL_MASK: u32 = 0xFF00;
            let ctl = unsafe { self.read_reg(sdmmc_regs::CLK_CTL_SWRST) };
            unsafe { self.write_reg(sdmmc_regs::CLK_CTL_SWRST, (ctl & !FREQ_SEL_MASK) | ((v as u32) << 8)) };
        }
        Ok(())
    }

    #[inline]
    unsafe fn read_reg(&self, offset: usize) -> u32 {
        core::ptr::read_volatile((self.base_vaddr + offset) as *const u32)
    }

    #[inline]
    unsafe fn write_reg(&self, offset: usize, value: u32) {
        core::ptr::write_volatile((self.base_vaddr + offset) as *mut u32, value);
    }

    /// 8 位写，与 LicheeRV sdhci_writeb 对齐（POWER/BLOCK_GAP/WAKE_UP/TIMEOUT 等）
    #[inline]
    unsafe fn write_reg_8(&self, offset: usize, value: u8) {
        core::ptr::write_volatile((self.base_vaddr + offset) as *mut u8, value);
    }

    /// 16 位写，用于 LicheeRV 对齐：先写 TRANSFER_MODE(0x0C) 再写 COMMAND(0x0E)。
    #[inline]
    unsafe fn write_reg_16(&self, offset: usize, value: u16) {
        core::ptr::write_volatile((self.base_vaddr + offset) as *mut u16, value);
    }

    /// 等待可以发命令：PRESENT_STS 中 CMD_INHIBIT、CMD_INHIBIT_DAT 为 0。
    ///
    /// PRESENT_STS 为只读，由硬件在“命令/数据完成且软件清除 NORM_AND_ERR_INT_STS”后自动清零 inhibit 位。
    /// 若一直不为 0，说明上一命令完成后未正确清除中断（应写完整读出的值到 NORM_AND_ERR_INT_STS），
    /// 或发新命令前未 clear_int_status。见 TRM 非数据传输指令流程、U-Boot sdhci.c 发命令前清 INT_STATUS。
    fn wait_not_inhibit(&self) -> Result<(), i32> {
        const CMD_INHIBIT: u32 = 1 << 0;
        const CMD_INHIBIT_DAT: u32 = 1 << 1;
        const MASK: u32 = CMD_INHIBIT | CMD_INHIBIT_DAT;
        // 纯「读寄存器 + sleep(1ms)」轮询，保证约 100ms 内超时且每 1ms 让出 CPU，避免慢 CPU 下 spin 导致长时间无 timeout
        for _ in 0..WAIT_INHIBIT_TIMEOUT_MS {
            let sts = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            if (sts & MASK) == 0 {
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let sts = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_not_inhibit: inhibit bits never cleared, PRESENT_STS=0x{:08x} (check cmd complete + clear_int)", sts);
        Err(-110) // -ETIMEDOUT
    }

    /// 等待 CMD 完成（NORM_AND_ERR_INT_STS 的 CMD_CMPL），并仅清除 CMD 相关位；遇错误返回 Err。
    /// 仅清除 CMD_CMPL（及错误位），不清除 BUF_WRDY/BUF_RRDY，否则 CMD53 写/读时下一句
    /// wait_buf_wr_ready/wait_buf_rd_ready 会永远等不到（TRM：BUF_WRDY 常与 CMD_CMPL 同时置位）。
    fn wait_cmd_complete(&self) -> Result<(), i32> {
        const INT_CMD_CMPL: u32 = 1 << 0;
        const INT_CMD_TIMEOUT: u32 = 1 << 16;
        const INT_CMD_CRC: u32 = 1 << 17;
        const INT_CMD_END_BIT: u32 = 1 << 18;
        const INT_CMD_INDEX: u32 = 1 << 19;
        const INT_CMD_ERR_MASK: u32 = INT_CMD_TIMEOUT | INT_CMD_CRC | INT_CMD_END_BIT | INT_CMD_INDEX;
        /// 只清除命令完成与错误位，保留 BUF_WRDY/BUF_RRDY 等数据路径状态
        const INT_CMD_CLEAR_MASK: u32 = INT_CMD_CMPL | INT_CMD_ERR_MASK;

        // 按「读寄存器 + sleep(1ms)」轮询，约 100ms 内超时，每 1ms 让出 CPU
        const POLL_MS: u32 = 100;
        for _ in 0..POLL_MS {
            let sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (sts & INT_CMD_ERR_MASK) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_cmd_complete: error INT_STS=0x{:08x}", sts);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts & INT_CMD_CLEAR_MASK) };
                if (sts & INT_CMD_TIMEOUT) != 0 {
                    return Err(-110); // ETIMEDOUT
                }
                if (sts & INT_CMD_CRC) != 0 {
                    return Err(-84);  // EILSEQ
                }
                if (sts & INT_CMD_END_BIT) != 0 {
                    return Err(-74);  // EBADMSG
                }
                return Err(-5);       // EIO
            }
            if (sts & INT_CMD_CMPL) != 0 {
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts & INT_CMD_CLEAR_MASK) };
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_cmd_complete: timeout INT_STS=0x{:08x} PRESENT_STS=0x{:08x} (inhibit_cmd={} inhibit_dat={})", int_sts, present, present & 1, (present >> 1) & 1);
        // 与 LicheeRV 一致：超时后须清理控制器状态，否则 inhibit 位不清除、后续 wait_not_inhibit 永远阻塞
        self.clear_int_status();
        self.reset_cmd_line();
        Err(-110)
    }

    /// 等待数据端口可读：按 TRM 步骤 (14) 等待 NORM_INT_STS[BUF_RRDY](bit5)=1，
    /// 或 PRESENT_STS.BUF_RD_ENABLE(bit11)=1；轮询带 1ms sleep 让出 CPU，避免持锁忙等卡住调度。
    fn wait_buf_rd_ready(&self) -> Result<(), i32> {
        const BUF_RRDY: u32 = 1 << 5;
        const BUF_RD_ENABLE: u32 = 1 << 11;
        const POLL_MS: u32 = 100;
        for _ in 0..POLL_MS {
            let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (ist & BUF_RRDY) != 0 {
                return Ok(());
            }
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            if (present & BUF_RD_ENABLE) != 0 {
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        // 位含义：NORM_INT bit5=BUF_RRDY bit8=CARD_INT bit4=BUF_WRDY bit0=CMD_CMPL bit1=XFER_CMPL bit16=CMD_TIMEOUT bit17=CMD_CRC_ERR
        // PRESENT bit0=CMD_INHIBIT bit1=CMD_INHIBIT_DAT bit10=BUF_WR_EN bit11=BUF_RD_EN
        log::error!(target: "wireless::bsp::sdio", "wait_buf_rd_ready: timeout NORM_INT_STS=0x{:08x} PRESENT_STS=0x{:08x} (BUF_RRDY=0 BUF_RD_EN={})", ist, present, (present >> 11) & 1);
        Err(-110)
    }

    /// 等待数据端口可写：按 TRM 步骤 (10) 等待 NORM_INT_STS[BUF_WRDY](bit4)=1，
    /// 或 PRESENT_STS.BUF_WR_ENABLE(bit10)=1（双条件，与 LicheeRV sdhci.c 轮询 PRESENT_STATE 一致）。
    /// 使用“每轮 1ms sleep”的轮询，让出 CPU，避免持锁忙等导致系统卡住（主线程 send_msg 时 busrx 无法运行）。
    fn wait_buf_wr_ready(&self) -> Result<(), i32> {
        const BUF_WRDY: u32 = 1 << 4;
        const BUF_WR_ENABLE: u32 = 1 << 10;
        const POLL_MS: u32 = 100; // 与 wait_not_inhibit 等对齐，给硬件足够时间
        for _ in 0..POLL_MS {
            let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (ist & BUF_WRDY) != 0 {
                return Ok(());
            }
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            if (present & BUF_WR_ENABLE) != 0 {
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_buf_wr_ready: timeout NORM_INT_STS=0x{:08x} PRESENT_STS=0x{:08x}", ist, present);
        Err(-110)
    }

    /// 等待 CMD53 数据阶段完成，等价于 LicheeRV sdhci_data_irq 的轮询版本。
    /// 判错顺序与 sdhci_data_irq 一致：先错误位再 INT_DATA_END；清除：写回 intmask。
    /// 差异与 quirk：LicheeRV 为中断驱动，DATA_END 与 DATA_TIMEOUT 常分两次 IRQ，本机为轮询且 SG2002
    /// 主机在数据完成时会同时置位 INT_DATA_END 与 INT_DATA_TIMEOUT(0x00108002)，故当两者同时置位时按成功处理。
    fn wait_xfer_complete(&self) -> Result<(), i32> {
        use sdhci_int::*;
        const POLL_MS: u32 = 100;
        for _ in 0..POLL_MS {
            let intmask = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (intmask & INT_DATA_END) != 0 && (intmask & INT_DATA_TIMEOUT) != 0 {
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Ok(());
            }
            if (intmask & INT_DATA_TIMEOUT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: DATA_TIMEOUT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-110);
            }
            if (intmask & INT_DATA_END_BIT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: DATA_END_BIT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-84);
            }
            if (intmask & INT_DATA_CRC) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: DATA_CRC INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-84);
            }
            if (intmask & INT_ADMA_ERROR) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: ADMA_ERROR INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-5);
            }
            if (intmask & INT_TIMEOUT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: CMD TIMEOUT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-110);
            }
            if (intmask & INT_DATA_END) != 0 {
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let final_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_xfer_complete: timeout INT_STS=0x{:08x}", final_sts);
        Err(-110)
    }

    /// 等待 DMA 数据阶段完成（与 LicheeRV sdhci_data_irq 中 INT_DMA_END 路径一致）。
    /// 先判错误位，再判 INT_DMA_END；清除写回读出的值。
    fn wait_dma_complete(&self) -> Result<(), i32> {
        use sdhci_int::*;
        const POLL_MS: u32 = 100;
        for _ in 0..POLL_MS {
            let intmask = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (intmask & INT_DATA_TIMEOUT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: DATA_TIMEOUT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-110);
            }
            if (intmask & INT_DATA_END_BIT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: DATA_END_BIT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-84);
            }
            if (intmask & INT_DATA_CRC) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: DATA_CRC INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-84);
            }
            if (intmask & INT_ADMA_ERROR) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: ADMA_ERROR INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-5);
            }
            if (intmask & INT_TIMEOUT) != 0 {
                log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: CMD TIMEOUT INT_STS=0x{:08x}", intmask);
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Err(-110);
            }
            if (intmask & INT_DMA_END) != 0 {
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, intmask) };
                return Ok(());
            }
            axtask::sleep(core::time::Duration::from_millis(1));
        }
        let final_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::error!(target: "wireless::bsp::sdio", "wait_dma_complete: timeout INT_STS=0x{:08x}", final_sts);
        Err(-110)
    }

    /// 清除中断状态（发送新命令前必须调用）：写回已置位位清对应位（W1C）
    fn clear_int_status(&self) {
        unsafe {
            let sts = self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS);
            if sts != 0 {
                self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts);
            }
        }
    }

    /// 等待 CMD 完成（IRQ 路径）：由 cmd53_*_blocks 在已设置 HOST_TRANSFER_CMD_PENDING 后调用。
    /// 等待条件：被 handle_sdhci_host_irq 在检测到 INT_CMD_MASK 且 CMD_PENDING 时，在清除 INT_STATUS、写入 HOST_CMD_RESULT 后调用 notify_one 唤醒。
    /// 超时时间：WAIT_CMD_DMA_IRQ_TIMEOUT_MS（1000ms），与 LicheeRV SDHCI_READ_STATUS_TIMEOUT 一致；超时后返回 Err(-110) 并清除 CMD_PENDING。
    fn wait_cmd_complete_irq(&self) -> Result<(), i32> {
        let dur = core::time::Duration::from_millis(WAIT_CMD_DMA_IRQ_TIMEOUT_MS);
        let timed_out = HOST_CMD_DONE_QUEUE.wait_timeout(dur);
        if timed_out {
            log::error!(target: "wireless::bsp::sdio", "wait_cmd_complete_irq: timeout");
            HOST_TRANSFER_CMD_PENDING.store(false, Ordering::SeqCst);
            return Err(-110);
        }
        let r = HOST_CMD_RESULT.load(Ordering::SeqCst);
        if r == 0 {
            Ok(())
        } else {
            Err(r)
        }
    }

    /// 等待 DMA 完成（IRQ 路径）：由 cmd53_*_blocks 在 CMD 完成后调用。等待条件：被 handle_sdhci_host_irq 在 DATA 分支（INT_DATA_END / INT_DATA_TIMEOUT / INT_DMA_END 等）处理并 notify_one 唤醒。超时同 WAIT_CMD_DMA_IRQ_TIMEOUT_MS。
    fn wait_dma_complete_irq(&self) -> Result<(), i32> {
        let dur = core::time::Duration::from_millis(WAIT_CMD_DMA_IRQ_TIMEOUT_MS);
        let timed_out = HOST_DMA_DONE_QUEUE.wait_timeout(dur);
        if timed_out {
            log::error!(target: "wireless::bsp::sdio", "wait_dma_complete_irq: timeout");
            HOST_TRANSFER_DMA_PENDING.store(false, Ordering::SeqCst);
            return Err(-110);
        }
        let r = HOST_DMA_RESULT.load(Ordering::SeqCst);
        if r == 0 {
            Ok(())
        } else {
            Err(r)
        }
    }

    /// 与 LicheeRV sdhci_set_transfer_irqs(DMA) 等价：关 PIO（DATA_AVAIL/SPACE_AVAIL），开 DMA_END|ADMA_ERROR，保留 DATA_END（整笔完成）；写 INT_ENABLE + SIGNAL_ENABLE。
    fn set_transfer_irqs_dma(&self) {
        use sdhci_int::{INT_ADMA_ERROR, INT_DATA_AVAIL, INT_DATA_END, INT_DMA_END, INT_SPACE_AVAIL};
        const PIO_IRQS: u32 = INT_SPACE_AVAIL | INT_DATA_AVAIL;
        const DMA_IRQS: u32 = INT_DMA_END | INT_DATA_END | INT_ADMA_ERROR; // LicheeRV dma_irqs = DMA_END|ADMA_ERROR，DATA_END 来自默认 ier
        let ier = unsafe {
            let current = self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN);
            (current & !PIO_IRQS) | DMA_IRQS
        };
        unsafe {
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN, ier);
            self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_SIG_EN, ier);
        }
    }

    // ---------- 与 LicheeRV drivers/mmc/core/sdio_irq.c + host/sdhci.c 完全等价的 SDIO IRQ 路径 ----------

    /// 与 LicheeRV sdio_get_pending_irqs 等价：CMD52 读 CCCR 0x05，返回 pending 字节（并应答卡中断）
    fn sdio_get_pending_irqs(&self) -> Result<u8, i32> {
        let pending = self.read_byte(SDIO_CCCR_INTX)?;
        Ok(pending)
    }

    /// 与 LicheeRV sdhci_enable_sdio_irq_nolock 等价：使能/关闭 CARD_INT 在 INT_STS_EN 与 INT_SIG_EN 中的位
    fn enable_sdio_irq(&self, enable: bool) {
        let en = if enable { CARD_INT } else { 0 };
        let sts_en = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS_EN) };
        let sig_en = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_SIG_EN) };
        unsafe {
            self.write_reg(
                sdmmc_regs::NORM_AND_ERR_INT_STS_EN,
                (sts_en & !CARD_INT) | en,
            );
            self.write_reg(
                sdmmc_regs::NORM_AND_ERR_INT_SIG_EN,
                (sig_en & !CARD_INT) | en,
            );
        }
    }

    /// 与 LicheeRV process_sdio_pending_irqs 等价：清 sdio_irq_pending，读 0x05 取 pending，按 bit 调 function 中断（当前无注册 handler，仅读 0x05）
    fn process_sdio_pending_irqs(&self) -> Result<(), i32> {
        use core::sync::atomic::Ordering;
        let _sdio_irq_pending = SDIO_IRQ_PENDING.swap(false, Ordering::SeqCst);
        let int_before = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let ret = self.sdio_get_pending_irqs();
        let int_after = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        log::warn!(target: "wireless::bsp::sdio", "process_sdio_pending_irqs: CMD52读0x05 结果{:?} | INT_STS 读前=0x{:08x} 读后=0x{:08x} CARD_INT消失? {}",
            ret, int_before, int_after, (int_after & CARD_INT) == 0);
        ret?;
        Ok(())
    }

    /// 与 LicheeRV host->ops->ack_sdio_irq 等价：重新使能 CARD_INT（读 0x05 后调用）
    fn ack_sdio_irq(&self) {
        self.enable_sdio_irq(true);
    }

    /// 与 LicheeRV sdio_run_irqs 等价：process_sdio_pending_irqs 后 ack_sdio_irq；由 sdio_irq_work 线程异步调用。
    pub(crate) fn sdio_run_irqs(&self) {
        let _ = self.process_sdio_pending_irqs();
        if !SDIO_IRQ_PENDING.load(core::sync::atomic::Ordering::SeqCst) {
            self.ack_sdio_irq();
        }
    }

    /// 与 LicheeRV sdio_signal_irq 等价：置 sdio_irq_pending，在无 workqueue 时由调用方紧接着调用 sdio_run_irqs
    fn sdio_signal_irq(&self) {
        SDIO_IRQ_PENDING.store(true, core::sync::atomic::Ordering::SeqCst);
    }

    /// 传输入口调用：若 CARD_INT 置位则仅 disable → signal → 入队 work（不在此处同步 run_irqs，与 LicheeRV sdio_signal_irq + queue_delayed_work 一致）。
    /// 返回 true 表示已入队，调用方应释放锁、wait_sdio_irq_work_done_timeout 后重试；返回 false 表示无 CARD_INT，可继续传输。
    fn process_sdio_pending_irqs_if_set(&self) -> bool {
        let sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        if (sts & CARD_INT) != 0 {
            self.enable_sdio_irq(false);
            self.sdio_signal_irq();
            super::irq::notify_sdio_irq_work();
            return true;
        }
        false
    }

    /// CMD52 单字节读（指定 function）。
    ///
    /// # 参数
    /// - `func`: Function number (0-7)
    /// - `reg`: 寄存器地址 (17 位)
    fn cmd52_read_func(&self, func: u32, reg: u32) -> Result<u8, i32> {
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ/完成路径按需清除
        let arg = ((func & 7) << 28) | ((reg & 0x1_FFFF) << 9);
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD52_XFER_MODE);
        }
        self.wait_cmd_complete()?;
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        // R5 响应：SD 模式下读数据在 resp 低字节（与 Linux sdio_ops.c *out = cmd.resp[0] & 0xFF 一致）
        if ((resp >> 16) & R5_ERROR_MASK) != 0 {
            log::error!(target: "wireless::bsp::sdio", "cmd52_read_func: R5 error, resp=0x{:08x}", resp);
            return Err(-5); // -EIO
        }
        Ok((resp & 0xFF) as u8)
    }

    /// CMD52 单字节写（指定 function）。
    fn cmd52_write_func(&self, func: u32, reg: u32, val: u8) -> Result<(), i32> {
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ/完成路径按需清除
        let arg = (1 << 31) | ((func & 7) << 28) | ((reg & 0x1_FFFF) << 9) | (val as u32);
        unsafe {
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD52_XFER_MODE);
        }
        self.wait_cmd_complete()?;
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        if ((resp >> 16) & R5_ERROR_MASK) != 0 {
            log::error!(target: "wireless::bsp::sdio", "cmd52_write_func: R5 error, resp=0x{:08x}", resp);
            return Err(-5);
        }
        Ok(())
    }

    /// CMD52 单字节读（与 LicheeRV sdio_cis.c 一致：CCCR/FBR/CIS 用 fn=0 + 17 位地址）。
    ///
    /// 调用方约定：CCCR(0x00-0xFF)、FBR/CIS 线性地址(0x109、0x0400、0x1000 等) 均用本接口，fn=0、addr 为 17 位。
    /// F1/F2 运行时寄存器(0x10A、0x112 等) 由上层通过 read_byte_at_func(1, reg) 访问，不经过本函数。
    fn cmd52_read(&self, addr: u32) -> Result<u8, i32> {
        self.cmd52_read_func(0, addr & 0x1_FFFF)
    }

    /// CMD52 单字节写（与 LicheeRV 一致：CCCR/FBR 等用 fn=0 + 17 位地址）。
    fn cmd52_write(&self, addr: u32, val: u8) -> Result<(), i32> {
        self.cmd52_write_func(0, addr & 0x1_FFFF, val)
    }

    /// CMD53 字节模式读：一次最多 512 字节。
    /// 交互顺序（便于对照日志）：1) 发 CMD53 读命令；2) wait_cmd_complete（命令阶段 R5）；3) 检查错误位；
    /// 4) 按 word 循环：wait_buf_rd_ready → 清 BUF_RRDY → 读 BUF_DATA；5) wait_xfer_complete；6) 读 R5。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 起始地址（func*0x100 + offset）。
    /// - `buf`: 数据写入的目标缓冲区，长度至少为 `count`。
    /// - `count`: 本次读取字节数，1..512。
    fn cmd53_read_chunk(&self, addr: u32, buf: &mut [u8], count: usize) -> Result<(), i32> {
        debug_assert!(count >= 1 && count <= CMD53_MAX_BYTES && count <= buf.len());
        let func = (addr >> 8) & 7;
        let reg = addr & 0xFF;
        log::info!(target: "wireless::bsp::sdio", "cmd53_read: addr=0x{:03x} count={} (F{} reg=0x{:02x})", addr, count, func, reg);
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ/完成路径按需清除
        if self.process_sdio_pending_irqs_if_set() {
            return Err(-11); // EAGAIN：已入队 work，调用方释放锁并 wait_sdio_irq_work_done 后重试
        }
        // 与 LicheeRV mmc_io_rw_extended 一致：arg 用 mmc_io_rw_extended_arg_byte；地址为函数内偏移（与 kernel 传 addr 一致，F1/F2 即 reg）
        let n = count as u32;
        let arg = sdhci::sdio_ops::mmc_io_rw_extended_arg_byte(false, func, reg, false, n);
        let blk_val: u32 = (1 << 16) | (count as u32);
        unsafe {
            self.write_reg(sdmmc_regs::BLK_SIZE_AND_CNT, blk_val);
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD53_READ_XFER_MODE);
        }
        // TRM 非 DMA 步骤 6-8：必须先等待并清除 CMD_CMPL，再进行 FIFO 读（步骤 14-17）
        if let Err(e) = self.wait_cmd_complete() {
            self.clear_int_status();
            self.reset_dat_line();
            return Err(e);
        }
        let sts_after_cmd = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let present_after_cmd = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::info!(target: "wireless::bsp::sdio", "cmd53_read: CMD_CMPL ok INT_STS=0x{:08x} PRESENT_STS=0x{:08x}", sts_after_cmd, present_after_cmd);
        const INT_XFER_ERR_READ: u32 = (1 << 20) | (1 << 21) | (1 << 22);
        let sts_r = sts_after_cmd;
        if (sts_r & (INT_XFER_ERR_READ | (1 << 16))) != 0 {
            unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts_r) };
            log::error!(target: "wireless::bsp::sdio", "cmd53_read_chunk: data/cmd error before FIFO read INT_STS=0x{:08x}", sts_r);
            self.clear_int_status();
            self.reset_dat_line();
            return Err(if (sts_r & (1 << 20)) != 0 { -110 } else if (sts_r & (1 << 16)) != 0 { -110 } else { -5 });
        }
        let words = (count + 3) / 4;
        log::info!(target: "wireless::bsp::sdio", "cmd53_read: start FIFO read {} words (addr=0x{:03x})", words, addr);
        const BUF_RRDY: u32 = 1 << 5; // TRM 步骤 14-16：等待 BUF_RRDY → 清除 → 读
        for i in 0..words {
            if let Err(e) = self.wait_buf_rd_ready() {
                // CMD53 读未完成就超时（如设备无数据），须清理状态否则 PRESENT_STS 的 CMD_INHIBIT_DAT 会一直置位，后续 CMD52 会卡在 wait_not_inhibit
                log::error!(target: "wireless::bsp::sdio", "cmd53_read: BUF_RRDY timeout at word {}/{} addr=0x{:03x}", i, words, addr);
                self.clear_int_status();
                self.reset_dat_line();
                return Err(e);
            }
            let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (ist & BUF_RRDY) != 0 {
                // 仅清除 BUF_RRDY，勿清除 XFER_CMPL（最后一块时两者可能同时置位）
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, BUF_RRDY) };
            }
            let word = unsafe { self.read_reg(sdmmc_regs::BUF_DATA) };
            let start = i * 4;
            let end = (start + 4).min(count);
            for j in start..end {
                buf[j] = (word >> ((j - start) * 8)) as u8;
            }
        }
        if let Err(e) = self.wait_xfer_complete() {
            self.clear_int_status();
            self.reset_dat_line();
            return Err(e);
        }
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        if ((resp >> 16) & R5_ERROR_MASK) != 0 {
            log::error!(target: "wireless::bsp::sdio", "cmd53_read_chunk: R5 error, resp=0x{:08x}", resp);
            self.clear_int_status();
            self.reset_dat_line();
            return Err(-5);
        }
        // 与 cmd53_write_chunk 一致：读完成后清中断、延时、等待 inhibit 清除，便于下一命令
        self.clear_int_status();
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        self.wait_not_inhibit()?;
        Ok(())
    }

    /// CMD53 字节模式写：一次最多 512 字节。
    /// 交互顺序：1) 发 CMD53 写命令；2) wait_cmd_complete；3) 清部分位后 wait_buf_wr_ready → 写 BUF_DATA；
    /// 4) wait_xfer_complete；5) 读 R5。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 起始地址（func*0x100 + offset）。
    /// - `buf`: 要写入的数据，长度至少为 `count`。
    /// - `count`: 本次写入字节数，1..512。
    fn cmd53_write_chunk(&self, addr: u32, buf: &[u8], count: usize) -> Result<(), i32> {
        debug_assert!(count >= 1 && count <= CMD53_MAX_BYTES && count <= buf.len());
        let func = (addr >> 8) & 7;
        let reg = addr & 0xFF;
        log::info!(target: "wireless::bsp::sdio", "cmd53_write: addr=0x{:03x} count={} (F{} reg=0x{:02x})", addr, count, func, reg);
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ/完成路径按需清除
        if self.process_sdio_pending_irqs_if_set() {
            return Err(-11); // EAGAIN：已入队 work，调用方释放锁并 wait_sdio_irq_work_done 后重试
        }
        // 与 LicheeRV mmc_io_rw_extended 一致：arg 用 mmc_io_rw_extended_arg_byte；地址为函数内偏移（与 kernel 传 addr 一致，F1/F2 即 reg）
        let n = count as u32;
        let arg = sdhci::sdio_ops::mmc_io_rw_extended_arg_byte(true, func, reg, false, n);
        let blk_val: u32 = (1 << 16) | (count as u32);
        unsafe {
            self.write_reg(sdmmc_regs::BLK_SIZE_AND_CNT, blk_val);
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
            self.write_reg(sdmmc_regs::XFER_MODE_AND_CMD, CMD53_WRITE_XFER_MODE);
        }
        // TRM 非 DMA 步骤 6-8：必须先等待并清除 CMD_CMPL，再进行 FIFO 写（步骤 10-13）
        if let Err(e) = self.wait_cmd_complete() {
            self.clear_int_status();
            self.reset_dat_line();
            return Err(e);
        }
        let sts_after_cmd = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        let present_after_cmd = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
        log::info!(target: "wireless::bsp::sdio", "cmd53_write: CMD_CMPL ok INT_STS=0x{:08x} PRESENT_STS=0x{:08x}", sts_after_cmd, present_after_cmd);
        // 部分主机在 CMD_CMPL 后仍保留 DATA_TIMEOUT/XFER_CMPL 等，会阻止 BUF_WRDY 置位；先清除这些位（保留 BUF_WRDY）
        const INT_XFER_ERR_MASK: u32 = (1 << 20) | (1 << 21) | (1 << 22);
        const CLEAR_BEFORE_FIFO: u32 = INT_XFER_ERR_MASK | (1 << 16) | (1 << 1); // DATA_* + CMD_TIMEOUT + XFER_CMPL
        let sts = sts_after_cmd;
        if (sts & CLEAR_BEFORE_FIFO) != 0 {
            unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, sts & CLEAR_BEFORE_FIFO) };
        }
        let sts2 = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
        if (sts2 & (INT_XFER_ERR_MASK | (1 << 16))) != 0 {
            log::error!(target: "wireless::bsp::sdio", "cmd53_write_chunk: data/cmd error before FIFO write INT_STS=0x{:08x}", sts2);
            self.clear_int_status();
            self.reset_dat_line();
            return Err(if (sts2 & (1 << 20)) != 0 { -110 } else if (sts2 & (1 << 16)) != 0 { -110 } else { -5 });
        }
        let words = (count + 3) / 4;
        const BUF_WRDY: u32 = 1 << 4; // TRM 步骤 10-12：等待 BUF_WRDY → 清除 → 写
        for i in 0..words {
            if let Err(e) = self.wait_buf_wr_ready() {
                // CMD53 写未完成就超时，须清理状态否则后续 CMD52 会卡在 wait_not_inhibit
                self.clear_int_status();
                self.reset_dat_line();
                return Err(e);
            }
            let ist = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            if (ist & BUF_WRDY) != 0 {
                // 仅清除 BUF_WRDY，勿清除 XFER_CMPL（最后一块时两者可能同时置位）
                unsafe { self.write_reg(sdmmc_regs::NORM_AND_ERR_INT_STS, BUF_WRDY) };
            }
            let start = i * 4;
            let end = (start + 4).min(count);
            let mut word = 0u32;
            for j in start..end {
                word |= (buf[j] as u32) << ((j - start) * 8);
            }
            unsafe { self.write_reg(sdmmc_regs::BUF_DATA, word) };
        }
        if let Err(e) = self.wait_xfer_complete() {
            self.clear_int_status();
            self.reset_dat_line();
            return Err(e);
        }
        let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
        if ((resp >> 16) & R5_ERROR_MASK) != 0 {
            log::error!(target: "wireless::bsp::sdio", "cmd53_write_chunk: R5 error, resp=0x{:08x}", resp);
            self.clear_int_status();
            self.reset_dat_line();
            return Err(-5);
        }
        // 与 LicheeRV SDHCI 一致：写完成后先清中断，再给控制器约 1ms 更新 PRESENT_STS，再等待 inhibit 清除（发下一命令前必须）
        self.clear_int_status();
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        self.wait_not_inhibit()?;
        Ok(())
    }

    /// CMD53 块模式多块写：一次 CMD53 传输 N×512 字节（与 LicheeRV mmc_io_rw_extended 单次请求一致），DMA 单缓冲。
    ///
    /// 寄存器顺序与 LicheeRV sdhci_prepare_data + sdhci_send_command 一致：SDMA_SADDR → HOST_CTRL(DMA_SEL) → BLK_SIZE_AND_CNT → ARGUMENT → XFER_MODE_AND_CMD。
    fn cmd53_write_blocks(&self, addr: u32, buf: &[u8], block_count: u32) -> Result<(), i32> {
        const BLOCKSIZE: usize = 512;
        let count = (block_count as usize) * BLOCKSIZE;
        assert!(block_count >= 1 && block_count <= 511 && buf.len() >= count);
        let func = (addr >> 8) & 7;
        let reg = addr & 0xFF;
        log::info!(target: "wireless::bsp::sdio", "cmd53_write: addr=0x{:03x} blocks={} ({} bytes, F{} reg=0x{:02x})", addr, block_count, count, func, reg);
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ 里按需清除
        if self.process_sdio_pending_irqs_if_set() {
            return Err(-11); // EAGAIN：已入队 work，调用方释放锁并 wait_sdio_irq_work_done 后重试
        }

        let (dma_ptr, dma_phys) = sdhci::alloc_dma_buffer(count).ok_or(-12)?;
        unsafe {
            let dma_slice = core::slice::from_raw_parts_mut(dma_ptr.as_ptr(), count);
            dma_slice.copy_from_slice(&buf[0..count]);
        }
        sdhci::cache::dma_flush_before_write(dma_ptr.as_ptr(), count);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        const HOST_CTRL1_DMA_SEL_MASK: u32 = 3 << 3;
        let host_ctrl_saved = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
        let arg = sdhci::sdio_ops::mmc_io_rw_extended_arg_block(true, func, reg, false, block_count);
        // 与 Linux sdhci_set_block_info 一致：BLOCK_SIZE 须含 sdma_boundary（SDHCI_MAKE_BLKSZ），否则多块 SDMA 不产生 DMA_END/DATA_END
        let blk_val: u32 = (block_count << 16) | SDHCI_BLOCK_SIZE_WORD;
        // 与 LicheeRV sdhci_prepare_data 顺序一致：initialize_data(字节数/bytes_xfered=0) → set_sdma_addr → config_dma → set_transfer_irqs → set_block_info
        HOST_DMA_BASE_PHYS.store(dma_phys as u32, Ordering::SeqCst);
        HOST_DMA_TOTAL.store(count, Ordering::SeqCst);
        HOST_DMA_BYTES_XFERRED.store(0, Ordering::SeqCst);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        unsafe {
            self.write_reg(sdmmc_regs::SDMA_SADDR, dma_phys as u32);
            self.write_reg(sdmmc_regs::HOST_CTRL1, host_ctrl_saved & !HOST_CTRL1_DMA_SEL_MASK);
        }
        self.set_transfer_irqs_dma();
        unsafe {
            self.write_reg(sdmmc_regs::BLK_SIZE_AND_CNT, blk_val);
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
        }
        // 与 LicheeRV sdhci.c 一致：有数据时发令前写 TIMEOUT_CONTROL=0x0E（BROKEN_TIMEOUT_VAL quirk）
        unsafe { self.write_reg_8(sdmmc_regs::TIMEOUT_CONTROL, 0x0E) };
        // 阶段1: 命令准备（已写 DMA 上下文 / SDMA_SADDR / HOST_CTRL1 / INT_EN+INT_SIG / BLK_SIZE_AND_CNT / ARGUMENT / TIMEOUT，未发 CMD）
        {
            let sdma = unsafe { self.read_reg(sdmmc_regs::SDMA_SADDR) };
            let blk = unsafe { self.read_reg(sdmmc_regs::BLK_SIZE_AND_CNT) };
            let arg_r = unsafe { self.read_reg(sdmmc_regs::ARGUMENT) };
            let hc1 = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: 命令准备 | SDMA_SADDR=0x{:08x} BLK_SIZE_AND_CNT=0x{:08x} ARGUMENT=0x{:08x} HOST_CTRL1=0x{:08x} PRESENT_STS=0x{:08x} INT_STS=0x{:08x}",
                sdma, blk, arg_r, hc1, present, int_sts);
        }
        HOST_TRANSFER_CMD_PENDING.store(true, Ordering::SeqCst);
        HOST_TRANSFER_DMA_PENDING.store(true, Ordering::SeqCst);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let xfer_mode_cmd = CMD53_WRITE_MULTI_XFER_MODE;
        unsafe {
            self.write_reg_16(sdmmc_regs::TRANSFER_MODE, (xfer_mode_cmd & 0xFFFF) as u16);
            self.write_reg_16(sdmmc_regs::COMMAND, (xfer_mode_cmd >> 16) as u16);
        }
        // 阶段2: 命令发出
        {
            let xfer_cmd = unsafe { self.read_reg(sdmmc_regs::XFER_MODE_AND_CMD) };
            let sdma = unsafe { self.read_reg(sdmmc_regs::SDMA_SADDR) };
            let blk = unsafe { self.read_reg(sdmmc_regs::BLK_SIZE_AND_CNT) };
            let arg_r = unsafe { self.read_reg(sdmmc_regs::ARGUMENT) };
            let hc1 = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: 命令发出 | XFER_MODE_AND_CMD=0x{:08x} SDMA_SADDR=0x{:08x} BLK=0x{:08x} ARG=0x{:08x} HOST_CTRL1=0x{:08x} PRESENT_STS=0x{:08x} INT_STS=0x{:08x} RESP31_0=0x{:08x}",
                xfer_cmd, sdma, blk, arg_r, hc1, present, int_sts, resp);
        }

        // 与 LicheeRV 一致：仅等 CMD 完成中断再等 DATA 完成中断（无轮询）
        let cmd_ok = self.wait_cmd_complete_irq();
        // 阶段3: 等待命令完成
        {
            let xfer_cmd = unsafe { self.read_reg(sdmmc_regs::XFER_MODE_AND_CMD) };
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
            let inhibit_cmd = present & 1;
            let inhibit_dat = (present >> 1) & 1;
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: 等待命令完成 | 命令发送{} | XFER_MODE_AND_CMD=0x{:08x} PRESENT_STS=0x{:08x} inhibit_cmd={} inhibit_dat={} INT_STS=0x{:08x} RESP31_0=0x{:08x}",
                if cmd_ok.is_ok() { "成功" } else { "失败" }, xfer_cmd, present, inhibit_cmd, inhibit_dat, int_sts, resp);
        }
        let res_after_cmd = match cmd_ok {
            Ok(()) => self.wait_dma_complete_irq(),
            Err(e) => Err(e),
        };
        // 阶段4: 等待数据完成（发出数据）
        {
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
            let data_ok = res_after_cmd.is_ok();
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: 等待数据完成 | 数据发送{} | PRESENT_STS=0x{:08x} INT_STS=0x{:08x} RESP31_0=0x{:08x}",
                if data_ok { "成功" } else { "失败" }, present, int_sts, resp);
        }
        let res = res_after_cmd.and_then(|_| {
            let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
            let r5_err = (resp >> 16) & R5_ERROR_MASK;
            let processed_ok = r5_err == 0;
            // 阶段5: R5 处理结果（数据接收/处理是否成功）
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: R5处理 | 处理{} | RESP31_0=0x{:08x} R5_ERROR_MASK&resp=0x{:x}",
                if processed_ok { "成功" } else { "失败" }, resp, r5_err);
            if r5_err != 0 {
                log::error!(target: "wireless::bsp::sdio", "cmd53_write_blocks: R5 error resp=0x{:08x}", resp);
                Err(-5)
            } else {
                Ok(())
            }
        });
        unsafe { self.write_reg(sdmmc_regs::HOST_CTRL1, host_ctrl_saved) };
        sdhci::release_dma_buffer();
        if let Err(e) = res {
            // 阶段6: 出错后
            let sdma = unsafe { self.read_reg(sdmmc_regs::SDMA_SADDR) };
            let blk = unsafe { self.read_reg(sdmmc_regs::BLK_SIZE_AND_CNT) };
            let arg_r = unsafe { self.read_reg(sdmmc_regs::ARGUMENT) };
            let xfer_cmd = unsafe { self.read_reg(sdmmc_regs::XFER_MODE_AND_CMD) };
            let hc1 = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
            let present = unsafe { self.read_reg(sdmmc_regs::PRESENT_STS) };
            let int_sts = unsafe { self.read_reg(sdmmc_regs::NORM_AND_ERR_INT_STS) };
            let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
            log::warn!(target: "wireless::bsp::sdio", "cmd53_write_blocks 阶段: 出错后 err={} | SDMA_SADDR=0x{:08x} BLK=0x{:08x} ARG=0x{:08x} XFER_CMD=0x{:08x} HOST_CTRL1=0x{:08x} PRESENT_STS=0x{:08x} INT_STS=0x{:08x} RESP31_0=0x{:08x}",
                e, sdma, blk, arg_r, xfer_cmd, hc1, present, int_sts, resp);
            self.clear_int_status();
            self.reset_cmd_line();
            self.reset_dat_line();
            return Err(e);
        }
        self.clear_int_status();
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        self.wait_not_inhibit()?;
        Ok(())
    }

    /// CMD53 块模式多块读：一次 CMD53 读 N×512 字节（与 LicheeRV mmc_io_rw_extended 单次请求一致），DMA 单缓冲。
    ///
    /// 寄存器顺序与 LicheeRV sdhci_prepare_data + sdhci_send_command 一致：SDMA_SADDR → HOST_CTRL(DMA_SEL) → BLK_SIZE_AND_CNT → ARGUMENT → XFER_MODE_AND_CMD；读完成后 dma_invalidate_after_read 再拷贝。
    fn cmd53_read_blocks(&self, addr: u32, buf: &mut [u8], block_count: u32) -> Result<(), i32> {
        const BLOCKSIZE: usize = 512;
        let count = (block_count as usize) * BLOCKSIZE;
        assert!(block_count >= 1 && block_count <= 511 && buf.len() >= count);
        let func = (addr >> 8) & 7;
        let reg = addr & 0xFF;
        log::info!(target: "wireless::bsp::sdio", "cmd53_read_blocks: addr=0x{:03x} blocks={} ({} bytes, F{} reg=0x{:02x})", addr, block_count, count, func, reg);
        self.wait_not_inhibit()?;
        // 与 LicheeRV 一致：不在 send_command 开头清 INT_STATUS，在 IRQ 里按需清除
        if self.process_sdio_pending_irqs_if_set() {
            return Err(-11); // EAGAIN：已入队 work，调用方释放锁并 wait_sdio_irq_work_done 后重试
        }

        let (dma_ptr, dma_phys) = sdhci::alloc_dma_buffer(count).ok_or(-12)?;
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        const HOST_CTRL1_DMA_SEL_MASK: u32 = 3 << 3;
        let host_ctrl_saved = unsafe { self.read_reg(sdmmc_regs::HOST_CTRL1) };
        let arg = sdhci::sdio_ops::mmc_io_rw_extended_arg_block(false, func, reg, false, block_count);
        // 与 Linux sdhci_set_block_info 一致：BLOCK_SIZE 须含 sdma_boundary（SDHCI_MAKE_BLKSZ）
        let blk_val: u32 = (block_count << 16) | SDHCI_BLOCK_SIZE_WORD;
        // 与 LicheeRV sdhci_prepare_data 顺序一致：DMA 上下文 → set_sdma_addr → config_dma → set_transfer_irqs → set_block_info
        HOST_DMA_BASE_PHYS.store(dma_phys as u32, Ordering::SeqCst);
        HOST_DMA_TOTAL.store(count, Ordering::SeqCst);
        HOST_DMA_BYTES_XFERRED.store(0, Ordering::SeqCst);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        unsafe {
            self.write_reg(sdmmc_regs::SDMA_SADDR, dma_phys as u32);
            self.write_reg(sdmmc_regs::HOST_CTRL1, host_ctrl_saved & !HOST_CTRL1_DMA_SEL_MASK);
        }
        self.set_transfer_irqs_dma();
        unsafe {
            self.write_reg(sdmmc_regs::BLK_SIZE_AND_CNT, blk_val);
            self.write_reg(sdmmc_regs::ARGUMENT, arg);
        }
        // 与 LicheeRV sdhci.c 一致：有数据时发令前写 TIMEOUT_CONTROL=0x0E
        unsafe { self.write_reg_8(sdmmc_regs::TIMEOUT_CONTROL, 0x0E) };
        HOST_TRANSFER_CMD_PENDING.store(true, Ordering::SeqCst);
        HOST_TRANSFER_DMA_PENDING.store(true, Ordering::SeqCst);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        let xfer_mode_cmd = CMD53_READ_MULTI_XFER_MODE;
        unsafe {
            self.write_reg_16(sdmmc_regs::TRANSFER_MODE, (xfer_mode_cmd & 0xFFFF) as u16);
            self.write_reg_16(sdmmc_regs::COMMAND, (xfer_mode_cmd >> 16) as u16);
        }
        let res = self.wait_cmd_complete_irq()
            .and_then(|_| self.wait_dma_complete_irq())
            .and_then(|_| {
                let resp = unsafe { self.read_reg(sdmmc_regs::RESP31_0) };
                if ((resp >> 16) & R5_ERROR_MASK) != 0 {
                    log::error!(target: "wireless::bsp::sdio", "cmd53_read_blocks: R5 error resp=0x{:08x}", resp);
                    Err(-5)
                } else {
                    Ok(())
                }
            });
        if res.is_ok() {
            sdhci::cache::dma_invalidate_after_read(dma_ptr.as_ptr(), count);
            unsafe {
                let dma_slice = core::slice::from_raw_parts(dma_ptr.as_ptr(), count);
                buf[0..count].copy_from_slice(dma_slice);
            }
        }
        unsafe { self.write_reg(sdmmc_regs::HOST_CTRL1, host_ctrl_saved) };
        sdhci::release_dma_buffer();
        if let Err(e) = res {
            self.clear_int_status();
            self.reset_cmd_line();
            self.reset_dat_line();
            return Err(e);
        }
        self.clear_int_status();
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        self.wait_not_inhibit()?;
        Ok(())
    }

    /// 单字节读：走 CMD52。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 地址，即 func*0x100 + 寄存器偏移。
    pub fn read_byte(&self, addr: u32) -> Result<u8, i32> {
        self.cmd52_read(addr)
    }

    /// 单字节读且 **始终 fn=0**（与 LicheeRV sdio_cis.c 一致：所有 CIS 相关 CMD52 用 fn=0 + 17 位地址）。
    pub fn read_byte_f0(&self, addr: u32) -> Result<u8, i32> {
        self.cmd52_read_func(0, addr & 0x1_FFFF)
    }

    /// 按指定 function 和 17 位寄存器地址读一字节（用于 F1 CIS：Linux MMC 用 func=1 读 F1 的 CIS）。
    pub fn read_byte_at_func(&self, func: u8, reg: u32) -> Result<u8, i32> {
        self.cmd52_read_func(func as u32, reg & 0x1_FFFF)
    }

    /// 单字节写：走 CMD52。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 地址（func*0x100 + 寄存器偏移），CCCR/FBR 用 fn=0+17 位。
    /// - `val`: 要写入的字节值。
    pub fn write_byte(&self, addr: u32, val: u8) -> Result<(), i32> {
        self.cmd52_write(addr, val)
    }

    /// 按指定 function 和寄存器地址写一字节（与 LicheeRV aicwf_sdio_writeb(sdiodev->func, reg, val) 一致）。
    pub fn write_byte_at_func(&self, func: u8, reg: u32, val: u8) -> Result<(), i32> {
        self.cmd52_write_func(func as u32, reg & 0x1_FFFF, val)
    }

    /// 设置指定 SDIO Function 的 block size（LicheeRV sdio_set_block_size(func, 512)：F1 aicsdio.c 1728，F2 func_msg）。
    /// 在启用该 function 后、首次 CMD53 块传输前调用，否则部分卡可能不响应数据阶段。
    pub fn set_block_size(&self, func: u32, size: u16) -> Result<(), i32> {
        self.cmd52_write_func(func, 0x10, (size & 0xFF) as u8)?;
        self.cmd52_write_func(func, 0x11, (size >> 8) as u8)?;
        Ok(())
    }

    /// 与 LicheeRV sdio_io_rw_ext_helper 读路径等价：size > 512 时块模式每次最多 511 块（DMA 大块传输），再字节模式扫尾。
    fn sdio_io_rw_ext_helper_read(&self, addr: u32, buf: &mut [u8], size: usize) -> Result<(), i32> {
        const BLOCKSIZE: usize = 512;
        const MAX_BLOCKS_PER_CMD: u32 = 511; // 与 Linux mmc_io_rw_extended 一致
        if size == 0 {
            return Ok(());
        }
        assert!(size <= buf.len());
        let mut offset = 0;
        if size > CMD53_MAX_BYTES {
            let mut remainder = size;
            while remainder >= BLOCKSIZE {
                let blocks = (remainder / BLOCKSIZE).min(MAX_BLOCKS_PER_CMD as usize) as u32;
                let chunk = blocks as usize * BLOCKSIZE;
                self.cmd53_read_blocks(addr, &mut buf[offset..offset + chunk], blocks)?;
                offset += chunk;
                remainder -= chunk;
            }
        }
        while offset < size {
            let n = (size - offset).min(CMD53_MAX_BYTES);
            self.cmd53_read_chunk(addr, &mut buf[offset..offset + n], n)?;
            offset += n;
        }
        Ok(())
    }

    /// 与 LicheeRV sdio_io_rw_ext_helper 写路径等价：size > 512 时块模式每次最多 511 块（DMA 大块传输），再字节模式扫尾。
    fn sdio_io_rw_ext_helper_write(&self, addr: u32, buf: &[u8], size: usize) -> Result<(), i32> {
        const BLOCKSIZE: usize = 512;
        const MAX_BLOCKS_PER_CMD: u32 = 511;
        if size == 0 {
            return Ok(());
        }
        assert!(size <= buf.len());
        let mut offset = 0;
        if size > CMD53_MAX_BYTES {
            let mut remainder = size;
            while remainder >= BLOCKSIZE {
                let blocks = (remainder / BLOCKSIZE).min(MAX_BLOCKS_PER_CMD as usize) as u32;
                let chunk = blocks as usize * BLOCKSIZE;
                self.cmd53_write_blocks(addr, &buf[offset..offset + chunk], blocks)?;
                offset += chunk;
                remainder -= chunk;
            }
        }
        while offset < size {
            let n = (size - offset).min(CMD53_MAX_BYTES);
            self.cmd53_write_chunk(addr, &buf[offset..offset + n], n)?;
            offset += n;
        }
        Ok(())
    }

    /// 块读：与 LicheeRV sdio_readsb 一致，走 sdio_io_rw_ext_helper（先多块 CMD53，再字节模式扫尾）。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 起始地址（func*0x100 + offset），FIFO 固定地址。
    /// - `buf`: 目标缓冲区，读入的字节数等于 `buf.len()`。
    pub fn read_block(&self, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }
        self.sdio_io_rw_ext_helper_read(addr, buf, len)?;
        Ok(len)
    }

    /// 用 CMD52 逐字节从指定 func 读入，不依赖 CMD53 BUF_RRDY。
    /// - func=1：F1 空间，rd_fifo 为单地址 FIFO（reg 0x08），须从同一 reg 重复读；其它用 reg, reg+1, ...
    /// - func=2：F2 空间，reg=(addr&0xFF)..
    fn read_cmd52(&self, func: u32, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
        let reg_base = addr & 0xFF;
        let rd_fifo_reg = 0x08u32; // reg::RD_FIFO_ADDR，单地址 FIFO（仅 F1）
        let is_fifo = (func == 1 && reg_base == rd_fifo_reg && buf.len() > 1);
        for (i, b) in buf.iter_mut().enumerate() {
            let r = if is_fifo { rd_fifo_reg } else { reg_base + i as u32 };
            *b = self.cmd52_read_func(func, r)?;
        }
        self.clear_int_status();
        crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
        Ok(buf.len())
    }

    /// 块写：与 LicheeRV sdio_writesb 一致，走 sdio_io_rw_ext_helper（先多块 CMD53，再字节模式扫尾）。
    ///
    /// # 参数
    /// - `addr`: 完整 SDIO 起始地址（func*0x100 + offset），FIFO 固定地址。
    /// - `buf`: 要写入的数据缓冲区，写入字节数等于 `buf.len()`。
    pub fn write_block(&self, addr: u32, buf: &[u8]) -> Result<usize, i32> {
        let len = buf.len();
        if len == 0 {
            return Ok(0);
        }
        self.sdio_io_rw_ext_helper_write(addr, buf, len)?;
        Ok(len)
    }

    /// 用 CMD52 逐字节写到指定 func，不依赖 CMD53 BUF_WRDY。
    /// - func=1：F1 空间 reg=(addr&0xFF)..（Aic8801 IPC wr_fifo 等），写后 clear_int_status + 延时并打 log。
    /// - func=2：F2 消息口等，reg=(addr&0xFF)..（如 0x207 则 reg 从 7 开始）。
    fn write_cmd52(&self, func: u32, addr: u32, buf: &[u8]) -> Result<usize, i32> {
        let reg_base = addr & 0xFF;
        if func == 1 {
            log::info!(target: "wireless::bsp::sdio", "write_cmd52: func=1 addr=0x{:03x} len={}", addr, buf.len());
        }
        for (i, &b) in buf.iter().enumerate() {
            self.cmd52_write_func(func, reg_base + i as u32, b)?;
        }
        if func == 1 {
            self.clear_int_status();
            crate::delay_spin_ms(WAIT_INHIBIT_DELAY_MS);
            log::info!(target: "wireless::bsp::sdio", "write_cmd52: func=1 done");
        }
        Ok(buf.len())
    }
}
