//! 固件配置
//! 对应 aic_bsp_main.c 中的固件定义

use crate::ProductId;

/// 芯片版本（与 LicheeRV aic_bsp_driver.h enum chip_rev 数值完全一致）
///
/// 8801 读 0x40500000 得 memdata，chip_rev = (memdata >> 16) & 0xFF；
/// LicheeRV 仅接受 U02(3)、U03(7)、U04(7)，U04 与 U03 同为 7。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChipRev {
    U01 = 1,
    U02 = 3,
    U03 = 7,
    // U04 在 LicheeRV 中与 U03 同值 7，Rust 枚举不可重复，逻辑上用 7 表示 U03/U04
}

impl ChipRev {
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(ChipRev::U01),
            3 => Some(ChipRev::U02),
            7 => Some(ChipRev::U03),
            _ => None,
        }
    }
}

/// 固件描述
#[derive(Debug, Clone)]
pub struct AicBspFirmware {
    pub desc: &'static str,
    pub bt_adid: &'static str,
    pub bt_patch: &'static str,
    pub bt_table: &'static str,
    pub wl_fw: &'static str,
    pub bt_ext_patch: Option<&'static str>,
}

/// 芯片模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum AicBspCpMode {
    #[default]
    Work = 0,
    Test = 1,
    Max,
}

/// 固件列表 - U02
pub const FW_U02: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(sdio u02)",
        bt_adid: "fw_adid.bin",
        bt_patch: "fw_patch.bin",
        bt_table: "fw_patch_table.bin",
        wl_fw: "fmacfw.bin",
        bt_ext_patch: None,
    },
    AicBspFirmware {
        desc: "rf test mode(sdio u02)",
        bt_adid: "fw_adid.bin",
        bt_patch: "fw_patch.bin",
        bt_table: "fw_patch_table.bin",
        wl_fw: "fmacfw_rf.bin",
        bt_ext_patch: None,
    },
];

/// 固件列表 - 8800DC U02
pub const FW_8800DC_U02: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800dc sdio u02)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "fmacfw_patch_8800dc_u02.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800dc sdio u02)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "lmacfw_rf_8800dc.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
];

/// 固件列表 - 8800D80 U02
pub const FW_8800D80_U02: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800d80 sdio u02)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "fmacfw_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800d80 sdio u02)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "lmacfw_rf_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
];

/// 固件列表 - U03（8801 chip_rev != U02 时用，与 LicheeRV fw_u03 对应）
pub const FW_U03: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(sdio u03)",
        bt_adid: "fw_adid.bin",
        bt_patch: "fw_patch.bin",
        bt_table: "fw_patch_table.bin",
        wl_fw: "fmacfw.bin",
        bt_ext_patch: None,
    },
    AicBspFirmware {
        desc: "rf test mode(sdio u03)",
        bt_adid: "fw_adid.bin",
        bt_patch: "fw_patch.bin",
        bt_table: "fw_patch_table.bin",
        wl_fw: "fmacfw_rf.bin",
        bt_ext_patch: None,
    },
];

/// 固件列表 - 8800DC U01
pub const FW_8800DC_U01: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800dc sdio u01)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "fmacfw_patch_8800dc_u02.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800dc sdio u01)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "lmacfw_rf_8800dc.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
];

/// 固件列表 - 8800DC H U02（is_chip_id_h 时）
pub const FW_8800DC_H_U02: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800dc h sdio u02)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "fmacfw_patch_8800dc_u02.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800dc h sdio u02)",
        bt_adid: "fw_adid_8800dc_u02.bin",
        bt_patch: "fw_patch_8800dc_u02.bin",
        bt_table: "fw_patch_table_8800dc_u02.bin",
        wl_fw: "lmacfw_rf_8800dc.bin",
        bt_ext_patch: Some("fw_patch_8800dc_u02_ext"),
    },
];

/// 固件列表 - 8800D80 U01
pub const FW_8800D80_U01: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800d80 sdio u01)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "fmacfw_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800d80 sdio u01)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "lmacfw_rf_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
];

/// 固件列表 - 8800D80 H U02
pub const FW_8800D80_H_U02: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800d80 h sdio u02)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "fmacfw_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800d80 h sdio u02)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "lmacfw_rf_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
];

/// 固件列表 - 8800D80X2
pub const FW_8800D80X2: [AicBspFirmware; 2] = [
    AicBspFirmware {
        desc: "normal work mode(8800d80x2)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "fmacfw_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
    AicBspFirmware {
        desc: "rf test mode(8800d80x2)",
        bt_adid: "fw_adid_8800d80_u02.bin",
        bt_patch: "fw_patch_8800d80_u02.bin",
        bt_table: "fw_patch_table_8800d80_u02.bin",
        wl_fw: "lmacfw_rf_8800d80_u02.bin",
        bt_ext_patch: Some("fw_patch_8800d80_u02_ext"),
    },
];

/// 根据 product_id 与 chip_rev 选择固件表（对应 LicheeRV aicbsp_driver_fw_init 内 aicbsp_firmware_list 赋值）
pub fn get_firmware_list(
    product_id: ProductId,
    chip_rev: u8,
    is_chip_id_h: bool,
) -> Option<&'static [AicBspFirmware; 2]> {
    match product_id {
        // 与 LicheeRV aic_bsp_driver.c 2019-2027：8801 chip_rev=(memdata>>16) 无 mask，仅接受 3/7，否则 return -1
        ProductId::Aic8801 => {
            if chip_rev != ChipRev::U02 as u8 && chip_rev != ChipRev::U03 as u8 {
                return None;
            }
            if chip_rev == ChipRev::U02 as u8 {
                Some(&FW_U02)
            } else {
                Some(&FW_U03)
            }
        }
        ProductId::Aic8800Dc | ProductId::Aic8800Dw => {
            let rev = chip_rev & 0x3F;
            if rev != ChipRev::U01 as u8 && rev != ChipRev::U02 as u8 && rev != ChipRev::U03 as u8 {
                return None;
            }
            if is_chip_id_h {
                Some(&FW_8800DC_H_U02)
            } else if rev == ChipRev::U01 as u8 {
                Some(&FW_8800DC_U01)
            } else {
                Some(&FW_8800DC_U02)
            }
        }
        ProductId::Aic8800D80 => {
            let rev = chip_rev & 0x3F;
            if is_chip_id_h {
                Some(&FW_8800D80_H_U02)
            } else if rev == ChipRev::U01 as u8 {
                Some(&FW_8800D80_U01)
            } else {
                Some(&FW_8800D80_U02)
            }
        }
        ProductId::Aic8800D80X2 => {
            let rev = chip_rev & 0x3F;
            if rev >= ChipRev::U03 as u8 + 8 {
                Some(&FW_8800D80X2)
            } else {
                None
            }
        }
    }
}
