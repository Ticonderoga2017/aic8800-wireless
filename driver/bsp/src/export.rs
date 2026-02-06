//! AIC BSP 导出接口
//! 对应 aic_bsp_export.h

/// 子系统类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AicBspSubsys {
    Bluetooth = 0,
    Wifi = 1,
}

/// 电源状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AicBspPwrState {
    Off = 0,
    On = 1,
}

/// 预留内存缓冲区ID
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SkBuffId {
    TxData = 0,
}

/// BSP 特性配置
#[derive(Debug, Clone, Default)]
pub struct AicBspFeature {
    pub hwinfo: i32,
    pub sdio_clock: u32,
    pub sdio_phase: u8,
    pub fwlog_en: bool,
    pub irqf: u8,
}

/// BSP 全局信息（对应 aic_bsp_main.c 中 aicbsp_info）
/// 用于保存 cpmode、hwinfo 等，供固件加载等使用
#[derive(Debug, Clone, Default)]
pub struct AicBspInfo {
    pub cpmode: u8,
    pub hwinfo_r: i32,
    pub hwinfo: i32,
    pub chip_rev: u8,
    pub fwlog_en: bool,
}

impl AicBspInfo {
    /// 用于静态初始化（const 上下文），与 Default::default() 同值
    pub const fn default_const() -> Self {
        Self {
            cpmode: 0,
            hwinfo_r: 0,
            hwinfo: 0,
            chip_rev: 0,
            fwlog_en: false,
        }
    }
}
