//! LicheeRV 芯片识别完整流程（C → Rust 移植）
//!
//! 对应 aicsdio.c：aicbsp_sdio_probe 入口的 vid/did 预检查 + aicwf_sdio_chipmatch。
//! 流程：读 CIS 得 (vid, did) → 预检查是否为本驱动支持的 AIC 卡 → chipmatch → ProductId。

use super::ops::SdioOps;
use super::types::{chipmatch, sdio_ids, ProductId};
use crate::sdio::cis::read_vendor_device;

/// 预检查：(vid, did) 是否为 LicheeRV aicbsp_sdio_probe 允许的卡
///
/// 与 aicsdio.c 283-291 行一致：仅当 (vid, did) 属于下列之一时允许继续，否则返回 -ENODEV。
/// - (SDIO_VENDOR_ID_AIC8801, SDIO_DEVICE_ID_AIC8801) 或 SDIO_DEVICE_ID_AIC8801_FUNC2
/// - (SDIO_VENDOR_ID_AIC8800DC, SDIO_DEVICE_ID_AIC8800DC)
/// - (SDIO_VENDOR_ID_AIC8800D80, SDIO_DEVICE_ID_AIC8800D80) 或 SDIO_DEVICE_ID_AIC8800D80_FUNC2
/// - (SDIO_VENDOR_ID_AIC8800D80X2, SDIO_DEVICE_ID_AIC8800D80X2)
#[inline]
pub fn is_known_aic_sdio(vid: u16, did: u16) -> bool {
    use sdio_ids::*;
    if vid == VENDOR_AIC8801 && (did == DEVICE_AIC8801 || did == DEVICE_AIC8801_FUNC2) {
        return true;
    }
    if vid == VENDOR_AIC8800DC && did == DEVICE_AIC8800DC {
        return true;
    }
    if vid == VENDOR_AIC8800D80 && (did == DEVICE_AIC8800D80 || did == DEVICE_AIC8800D80_FUNC2) {
        return true;
    }
    if vid == VENDOR_AIC8800D80X2 && did == DEVICE_AIC8800D80X2 {
        return true;
    }
    false
}

/// LicheeRV aicwf_sdio_chipmatch 的 Rust 等价： (vid, did) → ProductId
///
/// 与 aicsdio.c 235-255 行一致；已包含 AIC8801_FUNC2 / AIC8800D80_FUNC2 → 同一 ProductId。
#[inline]
pub fn aicwf_sdio_chipmatch(vid: u16, did: u16) -> Option<ProductId> {
    chipmatch(vid, did)
}

/// 从 SDIO CIS 执行完整芯片识别（LicheeRV 识别流程的完整移植）
///
/// 对应 LicheeRV 顺序：
/// 1. 内核/MMC 读 FBR/CIS → func->vendor, func->device（本实现用 read_vendor_device）；
/// 2. aicbsp_sdio_probe 内 if (vid/did 不在允许表) return -ENODEV（本实现用 is_known_aic_sdio）；
/// 3. aicwf_sdio_chipmatch(sdiodev, vid, did) → chipid（本实现用 aicwf_sdio_chipmatch）。
///
/// # 参数
/// - `ops`: 实现 SdioOps 的引用（如 CisReadOps），用于读 CCCR/FBR/CIS。
/// - `func_num`: 读哪个 function 的 FBR CIS，通常为 1（F1）。
///
/// # 返回
/// - `Ok(pid)`: 识别到的 ProductId；
/// - `Err(-1)`: (vid, did) 不在允许表（非本驱动支持的 AIC 卡）；
/// - `Err(-2)`: CIS 中无 CISTPL_MANFID 或读 CIS 失败；
/// - 其他负值：read_vendor_device 的错误码。
pub fn identify_chip_from_cis<O: SdioOps>(ops: &O, func_num: u8) -> Result<ProductId, i32> {
    let (vid, did) = read_vendor_device(ops, func_num)?;
    if !is_known_aic_sdio(vid, did) {
        log::error!(
            target: "wireless::bsp::sdio",
            "identify_chip_from_cis: vid=0x{:04x} did=0x{:04x} 不在 LicheeRV 允许表内 (aicbsp_sdio_probe 会 return -ENODEV)",
            vid, did
        );
        return Err(-1);
    }
    let pid = aicwf_sdio_chipmatch(vid, did).ok_or_else(|| {
        log::error!(
            target: "wireless::bsp::sdio",
            "identify_chip_from_cis: chipmatch 未匹配 vid=0x{:04x} did=0x{:04x}",
            vid, did
        );
        -1
    })?;
    log::info!(
        target: "wireless::bsp::sdio",
        "identify_chip_from_cis: vid=0x{:04x} did=0x{:04x} -> {:?} (align LicheeRV aicwf_sdio_chipmatch)",
        vid, did, pid
    );
    Ok(pid)
}
