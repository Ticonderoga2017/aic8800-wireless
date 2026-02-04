//! FDRV SDIO 总线与设备管理
//!
//! 对应 LicheeRV-Nano-Build aic8800_fdrv：
//! - aicwf_sdio.h / aicwf_sdio.c：aic_sdio_dev、aic_sdio_reg、SDIO 寄存器与常量
//! - aicwf_txrxif.h / aicwf_txrxif.c：aicwf_bus、aicwf_bus_ops、aicwf_tx_priv、aicwf_rx_priv
//! - sdio_host.h / sdio_host.c：sdio_host_env_tag、txdesc/host_id 管理
//! - ipc_shared.h：NX_TXQ_CNT、NX_TXDESC_CNT 等
//!
//! 程序逻辑（LicheeRV aicwf_sdio_probe）：
//! 1. 分配 aicwf_bus、aic_sdio_dev
//! 2. aicwf_sdio_chipmatch(vid, did) → chipid
//! 3. sdiodev->func、sdiodev->bus_if、bus_if->bus_priv.sdio = sdiodev
//! 4. aicwf_sdio_func_init / aicwf_sdiov3_func_init（按 chipid）
//! 5. aicwf_sdio_bus_init(sdiodev) → aicwf_bus_init（起 bustx/busrx 线程）、填 bus_ops
//! 6. aicwf_rwnx_sdio_platform_init(sdiodev) → rwnx_platform_init → rwnx_cfg80211_init
//! 7. aicwf_hostif_ready()
//!
//! StarryOS 侧：无 sdio_func/kthread，由平台提供 SdioOps；本模块提供数据结构与常量对齐，
//! 实际收发通过 BSP SdioOps 或 fdrv SdioHostOps 完成。

use core::result::Result;

use bsp::ProductId;

// =============================================================================
// 常量（与 LicheeRV aicwf_sdio.h、ipc_shared.h 对齐）
// =============================================================================

/// FDRV SDIO 驱动名（对应 AICWF_SDIO_NAME）
#[allow(dead_code)]
pub const AICWF_SDIO_NAME: &str = "aicwf_sdio";

/// SDIO 块大小（对应 SDIOWIFI_FUNC_BLOCKSIZE）
pub const SDIOWIFI_FUNC_BLOCKSIZE: u32 = 512;

/// 缓冲区大小（对应 BUFFER_SIZE）
pub const SDIO_BUFFER_SIZE: usize = 1536;

/// 尾长度（对应 TAIL_LEN）
pub const SDIO_TAIL_LEN: usize = 4;

/// 电源控制间隔（对应 SDIOWIFI_PWR_CTRL_INTERVAL）
#[allow(dead_code)]
pub const SDIOWIFI_PWR_CTRL_INTERVAL: u32 = 30;

/// 流控重试次数（对应 FLOW_CTRL_RETRY_COUNT）
#[allow(dead_code)]
pub const FLOW_CTRL_RETRY_COUNT: u32 = 50;

/// 命令缓冲区最大长度（对应 CMD_BUF_MAX in aicwf_txrxif.h）
#[allow(dead_code)]
pub const CMD_BUF_MAX: usize = 1536;

/// 数据 TX 块大小（对应 TXPKT_BLOCKSIZE）
#[allow(dead_code)]
pub const TXPKT_BLOCKSIZE: u32 = 512;

/// 最大聚合 TX 长度（对应 MAX_AGGR_TXPKT_LEN）
#[allow(dead_code)]
pub const MAX_AGGR_TXPKT_LEN: usize = 1536 * 64;

/// TX 队列数（对应 NX_TXQ_CNT，Makefile 中 4 或 5）
pub const NX_TXQ_CNT: usize = 4;

/// 每队列 TX 描述符数（LicheeRV 为 NX_TXDESC_CNT0..4：8,64,64,32 或 +8）
/// 此处取统一上限，便于数组定义
pub const NX_TXDESC_CNT_MAX: usize = 64;

/// SDIO 睡眠/激活状态（对应 SDIO_SLEEP_ST / SDIO_ACTIVE_ST）
pub const SDIO_SLEEP_ST: u32 = 0;
pub const SDIO_ACTIVE_ST: u32 = 1;

// =============================================================================
// SDIO 寄存器映射（对应 struct aic_sdio_reg）
// =============================================================================

/// SDIO 寄存器偏移（按芯片可能使用 V1/V2 或 V3）
#[derive(Debug, Clone, Copy, Default)]
pub struct SdioReg {
    pub bytemode_len_reg: u8,
    pub intr_config_reg: u8,
    pub sleep_reg: u8,
    pub wakeup_reg: u8,
    pub flow_ctrl_reg: u8,
    pub flowctrl_mask_reg: u8,
    pub register_block: u8,
    pub bytemode_enable_reg: u8,
    pub block_cnt_reg: u8,
    pub misc_int_status_reg: u8,
    pub rd_fifo_addr: u8,
    pub wr_fifo_addr: u8,
}

impl SdioReg {
    /// V1/V2 芯片（8801/8800DC/DW）默认寄存器映射（对应 aicwf_sdio_reg_init）
    pub fn v1_v2_default() -> Self {
        Self {
            bytemode_len_reg: 0x02,
            intr_config_reg: 0x04,
            sleep_reg: 0x05,
            wakeup_reg: 0x09,
            flow_ctrl_reg: 0x0A,
            flowctrl_mask_reg: 0x7F,
            register_block: 0x0B,
            bytemode_enable_reg: 0x11,
            block_cnt_reg: 0x12,
            misc_int_status_reg: 0,
            rd_fifo_addr: 0x08,
            wr_fifo_addr: 0x07,
        }
    }

    /// V3 芯片（8800D80/D80X2）默认寄存器映射
    pub fn v3_default() -> Self {
        Self {
            bytemode_len_reg: 0x05,
            intr_config_reg: 0,
            sleep_reg: 0,
            wakeup_reg: 0,
            flow_ctrl_reg: 0x03,
            flowctrl_mask_reg: 0,
            register_block: 0,
            bytemode_enable_reg: 0x07,
            block_cnt_reg: 0,
            misc_int_status_reg: 0x04,
            rd_fifo_addr: 0x0F,
            wr_fifo_addr: 0x10,
        }
    }

    /// 按产品 ID 选择寄存器集
    pub fn for_product(chipid: ProductId) -> Self {
        match chipid {
            ProductId::Aic8800D80 | ProductId::Aic8800D80X2 => Self::v3_default(),
            _ => Self::v1_v2_default(),
        }
    }
}

// =============================================================================
// 总线状态（对应 enum aicwf_bus_state）
// =============================================================================

/// 总线状态（对应 aicwf_txrxif.h aicwf_bus_state）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum BusState {
    Down = 0,
    Up = 1,
}

// =============================================================================
// 总线操作 trait（对应 struct aicwf_bus_ops）
// =============================================================================

/// 总线操作（对应 aicwf_bus_ops）
/// 平台实现 start/stop/txdata/txmsg，内部可调用 BSP SdioOps
pub trait BusOps {
    /// 启动总线（claim host、使能中断等）
    fn start(&self) -> Result<(), i32>;
    /// 停止总线（release host、停线程等）
    fn stop(&self) -> Result<(), i32>;
    /// 发送数据（skb 等价：payload + len）
    fn txdata(&self, buf: &[u8]) -> Result<usize, i32>;
    /// 发送 LMAC 消息（cmd_buf 等）
    fn txmsg(&self, msg: &[u8]) -> Result<usize, i32>;
}

// =============================================================================
// SDIO Host 环境（对应 struct sdio_host_env_tag）
// =============================================================================

/// 每队列 host_id 环（LicheeRV 中 tx_host_id[queue_idx][free_idx % SDIO_TXDESC_CNT]）
const TXDESC_PER_QUEUE: usize = NX_TXDESC_CNT_MAX;

/// SDIO Host 环境（对应 sdio_host.h sdio_host_env_tag）
/// 用于 TX 描述符 / host_id 追踪，E2A TXCFM 时按 used_idx 取回 host_id
#[derive(Debug)]
pub struct SdioHostEnv {
    /// 每队列下一个空闲描述符下标（push 时写入，cfm 时不用）
    pub txdesc_free_idx: [u32; NX_TXQ_CNT],
    /// 每队列下一个已用描述符下标（cfm 时推进）
    pub txdesc_used_idx: [u32; NX_TXQ_CNT],
    /// 每队列 host_id 环（对应 tx_host_id[queue_idx][idx % N]）
    pub tx_host_id: [[u64; TXDESC_PER_QUEUE]; NX_TXQ_CNT],
    /// 附加上下文（对应 pthis，如 rwnx_hw）
    pub pthis: Option<*const ()>,
}

impl Default for SdioHostEnv {
    fn default() -> Self {
        Self {
            txdesc_free_idx: [0; NX_TXQ_CNT],
            txdesc_used_idx: [0; NX_TXQ_CNT],
            tx_host_id: [[0; TXDESC_PER_QUEUE]; NX_TXQ_CNT],
            pthis: None,
        }
    }
}

impl SdioHostEnv {
    /// 初始化（对应 aicwf_sdio_host_init）
    pub fn init(&mut self, pthis: Option<*const ()>) {
        *self = Self::default();
        self.pthis = pthis;
    }

    /// Push 一个 host_id 到指定队列（对应 aicwf_sdio_host_txdesc_push）
    #[inline]
    pub fn txdesc_push(&mut self, queue_idx: usize, host_id: u64) {
        if queue_idx >= NX_TXQ_CNT {
            return;
        }
        let free = self.txdesc_free_idx[queue_idx] as usize % TXDESC_PER_QUEUE;
        self.tx_host_id[queue_idx][free] = host_id;
        self.txdesc_free_idx[queue_idx] = self.txdesc_free_idx[queue_idx].wrapping_add(1);
    }

    /// TX CFM 处理：根据 data 更新 used_idx 并返回对应 host_id（对应 aicwf_sdio_host_tx_cfm_handler 语义）
    /// 返回 (queue_idx, host_id)；实际 E2A 解析由上层根据 msg 完成
    #[inline]
    pub fn tx_cfm_advance(&mut self, queue_idx: usize) -> Option<u64> {
        if queue_idx >= NX_TXQ_CNT {
            return None;
        }
        let used = self.txdesc_used_idx[queue_idx] as usize % TXDESC_PER_QUEUE;
        let host_id = self.tx_host_id[queue_idx][used];
        self.txdesc_used_idx[queue_idx] = self.txdesc_used_idx[queue_idx].wrapping_add(1);
        Some(host_id)
    }
}

// =============================================================================
// FDRV SDIO 设备（对应 struct aic_sdio_dev 核心字段）
// =============================================================================

/// FDRV 侧 SDIO 设备上下文（对应 aic_sdio_dev）
/// 持有芯片 ID、寄存器映射、状态；bus 与 rwnx_hw 由平台在 probe 时挂接
#[derive(Debug)]
pub struct SdioDev {
    /// 芯片 ID（chipmatch 结果）
    pub chipid: ProductId,
    /// SDIO 寄存器映射（按芯片选 V1/V2 或 V3）
    pub sdio_reg: SdioReg,
    /// 电源/活动状态（SDIO_SLEEP_ST / SDIO_ACTIVE_ST）
    pub state: u32,
}

impl SdioDev {
    /// 从 chipid 构造（对应 aicwf_sdio_chipmatch 之后）
    pub fn new(chipid: ProductId) -> Self {
        let sdio_reg = SdioReg::for_product(chipid);
        Self {
            chipid,
            sdio_reg,
            state: SDIO_ACTIVE_ST,
        }
    }

    /// 设为睡眠状态
    pub fn set_sleep(&mut self) {
        self.state = SDIO_SLEEP_ST;
    }

    /// 设为激活状态
    pub fn set_active(&mut self) {
        self.state = SDIO_ACTIVE_ST;
    }

    /// 是否 V3 芯片（8800D80/D80X2）
    pub fn is_v3(&self) -> bool {
        matches!(self.chipid, ProductId::Aic8800D80 | ProductId::Aic8800D80X2)
    }
}

// =============================================================================
// FDRV SDIO 注册/探测流程（占位，对应 aicwf_sdio_register / aicwf_sdio_probe）
// =============================================================================

/// FDRV SDIO 驱动注册（对应 aicwf_sdio_register）
/// LicheeRV：sdio_register_driver(&aicwf_sdio_driver) 或直接 aicwf_sdio_probe_(get_sdio_func(), NULL)
/// StarryOS：无内核 SDIO 总线；由平台在“SDIO 已就绪”时调用 aicwf_sdio_probe_equiv
#[inline]
pub fn aicwf_sdio_register_equiv() {
    log::debug!(target: "wireless::fdrv::sdio_bus", "aicwf_sdio_register_equiv (no kernel sdio, platform triggers probe)");
}

/// FDRV SDIO 探测等价：从已得到的 chipid 创建 SdioDev，并执行平台初始化（rwnx_platform_init 等价）
/// 参数：chipid 由 BSP probe 或平台检测得到；返回 SdioDev 供上层挂 rwnx_hw、bus_ops
pub fn aicwf_sdio_probe_equiv(chipid: ProductId) -> SdioDev {
    log::info!(target: "wireless::fdrv::sdio_bus", "aicwf_sdio_probe_equiv chipid={:?}", chipid);
    SdioDev::new(chipid)
}

/// FDRV SDIO 驱动注销（对应 aicwf_sdio_exit）
#[inline]
pub fn aicwf_sdio_exit_equiv() {
    log::debug!(target: "wireless::fdrv::sdio_bus", "aicwf_sdio_exit_equiv");
}
