//! AIC8800 SDIO 类型与常量
//! 对照 LicheeRV-Nano-Build aic8800_bsp/aicsdio.h、aic8800_fdrv/aicwf_sdio.h

/// SDIO Vendor/Device ID（aicsdio.c aicwf_sdio_chipmatch、aicwf_sdio.h）
pub mod sdio_ids {
    /// AIC8801
    pub const VENDOR_AIC8801: u16 = 0x5449;
    pub const DEVICE_AIC8801: u16 = 0x0145;
    pub const DEVICE_AIC8801_FUNC2: u16 = 0x0146;
    /// AIC8800DC
    pub const VENDOR_AIC8800DC: u16 = 0xc8a1;
    pub const DEVICE_AIC8800DC: u16 = 0xc08d;
    /// AIC8800D80
    pub const VENDOR_AIC8800D80: u16 = 0xc8a1;
    pub const DEVICE_AIC8800D80: u16 = 0x0082;
    pub const DEVICE_AIC8800D80_FUNC2: u16 = 0x0182;
    /// AIC8800D80X2
    pub const VENDOR_AIC8800D80X2: u16 = 0xc8a1;
    pub const DEVICE_AIC8800D80X2: u16 = 0x2082;
}

/// SDIO Vendor/Device ID (通用，对应 aicsdio.h 旧常量)
#[allow(dead_code)]
pub const SDIO_VENDOR_ID_AIC: u16 = 0x8800;
#[allow(dead_code)]
pub const SDIO_DEVICE_ID_AIC: u16 = 0x0001;

/// AIC8800 V1/V2 寄存器偏移（aicsdio.h SDIOWIFI_*，aicwf_sdio_reg_init 用于 8801/8800DC/8800DW）
pub mod reg {
    pub const BYTEMODE_LEN: u8 = 0x02;       // SDIOWIFI_BYTEMODE_LEN_REG
    pub const INTR_CONFIG: u8 = 0x04;        // SDIOWIFI_INTR_CONFIG_REG
    pub const SLEEP: u8 = 0x05;              // SDIOWIFI_SLEEP_REG
    pub const WAKEUP: u8 = 0x09;            // SDIOWIFI_WAKEUP_REG
    pub const FLOW_CTRL: u8 = 0x0A;         // SDIOWIFI_FLOW_CTRL_REG
    pub const REGISTER_BLOCK: u8 = 0x0B;    // SDIOWIFI_REGISTER_BLOCK
    pub const BYTEMODE_ENABLE: u8 = 0x11;   // SDIOWIFI_BYTEMODE_ENABLE_REG
    pub const BLOCK_CNT: u8 = 0x12;         // SDIOWIFI_BLOCK_CNT_REG
    pub const FLOWCTRL_MASK: u8 = 0x7F;     // SDIOWIFI_FLOWCTRL_MASK_REG
    pub const WR_FIFO_ADDR: u8 = 0x07;      // SDIOWIFI_WR_FIFO_ADDR
    pub const RD_FIFO_ADDR: u8 = 0x08;      // SDIOWIFI_RD_FIFO_ADDR
}

/// AIC8800 V3 寄存器偏移（aicsdio.h SDIOWIFI_*_V3，用于 8800D80/8800D80X2）
pub mod reg_v3 {
    pub const INTR_ENABLE: u8 = 0x00;        // SDIOWIFI_INTR_ENABLE_REG_V3
    pub const INTR_PENDING: u8 = 0x01;      // SDIOWIFI_INTR_PENDING_REG_V3
    pub const INTR_TO_DEVICE: u8 = 0x02;    // SDIOWIFI_INTR_TO_DEVICE_REG_V3
    pub const FLOW_CTRL_Q1: u8 = 0x03;      // SDIOWIFI_FLOW_CTRL_Q1_REG_V3
    pub const MISC_INT_STATUS: u8 = 0x04;   // SDIOWIFI_MISC_INT_STATUS_REG_V3
    pub const BYTEMODE_LEN: u8 = 0x05;      // SDIOWIFI_BYTEMODE_LEN_REG_V3
    pub const BYTEMODE_LEN_MSB: u8 = 0x06;   // SDIOWIFI_BYTEMODE_LEN_MSB_REG_V3
    pub const BYTEMODE_ENABLE: u8 = 0x07;   // SDIOWIFI_BYTEMODE_ENABLE_REG_V3
    pub const MISC_CTRL: u8 = 0x08;         // SDIOWIFI_MISC_CTRL_REG_V3
    pub const FLOW_CTRL_Q2: u8 = 0x09;      // SDIOWIFI_FLOW_CTRL_Q2_REG_V3
    pub const CLK_TEST_RESULT: u8 = 0x0A;   // SDIOWIFI_CLK_TEST_RESULT_REG_V3
    pub const RD_FIFO_ADDR: u8 = 0x0F;      // SDIOWIFI_RD_FIFO_ADDR_V3
    pub const WR_FIFO_ADDR: u8 = 0x10;      // SDIOWIFI_WR_FIFO_ADDR_V3
}

/// SDIO 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdioState {
    Sleep = 0,
    Active = 1,
}

/// 产品ID
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ProductId {
    Aic8801 = 0,
    Aic8800Dc,
    Aic8800Dw,
    Aic8800D80,
    Aic8800D80X2,
}

/// SDIO 块大小 (对应 SDIOWIFI_FUNC_BLOCKSIZE)
#[allow(dead_code)]
pub const SDIO_FUNC_BLOCKSIZE: u32 = 512;

/// SDIO 缓冲区大小
#[allow(dead_code)]
pub const BUFFER_SIZE: usize = 1536;

/// SDIO 尾长度
#[allow(dead_code)]
pub const TAIL_LEN: usize = 4;

/// SDIO 数据类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SdioType {
    Data = 0x00,
    Cfg = 0x10,
    CfgCmdRsp = 0x11,
    CfgDataCfm = 0x12,
}

/// **芯片检测（chipmatch）** — 根据 SDIO 读出的 vendor/device ID 得到 ProductId
///
/// 对应 LicheeRV aicsdio.c aicwf_sdio_chipmatch。
/// 在 Linux 上 vid/did 由内核在枚举 SDIO 卡时从 FBR（Function Basic Registers）读出；
/// StarryOS 上可读 F0 FBR 0x08（vendor）、0x0A（device）后调用本函数。
///
/// # 参数
/// - `vid`: SDIO 厂商 ID（FBR 0x08）。
/// - `did`: SDIO 设备 ID（FBR 0x0A）。
///
/// 返回 `Some(pid)` 表示识别为 AIC 芯片；`None` 表示非本驱动支持的型号。
#[inline]
pub fn chipmatch(vid: u16, did: u16) -> Option<ProductId> {
    use sdio_ids::*;
    if vid == VENDOR_AIC8801 && did == DEVICE_AIC8801 {
        return Some(ProductId::Aic8801);
    }
    if vid == VENDOR_AIC8800DC && did == DEVICE_AIC8800DC {
        return Some(ProductId::Aic8800Dc);
    }
    if vid == VENDOR_AIC8800D80 && did == DEVICE_AIC8800D80 {
        return Some(ProductId::Aic8800D80);
    }
    if vid == VENDOR_AIC8800D80X2 && did == DEVICE_AIC8800D80X2 {
        return Some(ProductId::Aic8800D80X2);
    }
    None
}
