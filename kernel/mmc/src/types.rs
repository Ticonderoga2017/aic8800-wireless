//! MMC/SDIO 通用类型
//!
//! 对应 Linux：include/linux/mmc/card.h、host.h、sdio_func.h、sdio_ids.h

/// SDIO 设备 ID（用于驱动匹配，对应 struct sdio_device_id）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdioDeviceId {
    /// 标准接口类，SDIO_ANY_ID(0xff) 表示任意
    pub class: u8,
    /// 厂商 ID，SDIO_ANY_ID(0xffff) 表示任意
    pub vendor: u16,
    /// 设备 ID，SDIO_ANY_ID(0xffff) 表示任意
    pub device: u16,
}

/// 匹配任意类/厂商/设备（Linux SDIO_ANY_ID）
pub const SDIO_ANY_ID: u8 = 0xff;
pub const SDIO_ANY_ID_U16: u16 = 0xffff;

impl SdioDeviceId {
    pub const fn any() -> Self {
        Self {
            class: SDIO_ANY_ID,
            vendor: SDIO_ANY_ID_U16,
            device: SDIO_ANY_ID_U16,
        }
    }

    pub const fn new(class: u8, vendor: u16, device: u16) -> Self {
        Self { class, vendor, device }
    }

    /// 是否匹配另一个 ID（class/vendor/device 任一为 ANY 则该字段恒匹配）
    pub fn matches(&self, other: &SdioDeviceId) -> bool {
        (self.class == SDIO_ANY_ID || self.class == other.class)
            && (self.vendor == SDIO_ANY_ID_U16 || self.vendor == other.vendor)
            && (self.device == SDIO_ANY_ID_U16 || self.device == other.device)
    }
}

/// 总线宽度（对应 MMC_BUS_WIDTH_*）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum MmcBusWidth {
    #[default]
    OneBit = 0,
    FourBit = 2,
    EightBit = 3,
}

/// Host 接口配置（对应 struct mmc_ios）
#[derive(Debug, Clone, Copy, Default)]
pub struct MmcIos {
    /// 时钟频率 Hz
    pub clock: u32,
    /// 总线宽度
    pub bus_width: MmcBusWidth,
}

impl MmcIos {
    pub const fn default_legacy() -> Self {
        Self {
            clock: 400_000,
            bus_width: MmcBusWidth::OneBit,
        }
    }
}

/// SDIO 标准接口类（Linux SDIO_CLASS_*）
pub mod sdio_class {
    pub const NONE: u8 = 0x00;
    pub const UART: u8 = 0x01;
    pub const BT_A: u8 = 0x02;
    pub const BT_B: u8 = 0x03;
    pub const GPS: u8 = 0x04;
    pub const CAMERA: u8 = 0x05;
    pub const PHS: u8 = 0x06;
    pub const WLAN: u8 = 0x07;
    pub const ATA: u8 = 0x08;
    pub const BT_AMP: u8 = 0x09;
}

/// 常用块大小（与 Linux SDIOWIFI_FUNC_BLOCKSIZE 一致）
pub const SDIO_FUNC_BLOCKSIZE_DEFAULT: u16 = 512;
