//! 本地固件按名称读取
//!
//! 固件按名称在 `StarryOS/wireless/firmware` 查找：
//! - 默认 feature `embed_firmware_8801`：仅嵌入 fmacfw.bin、fmacfw_patch.bin（8801 必需），需复制到 wireless/firmware/
//! - feature `embed_firmware`：嵌入全部 .bin（依赖 embed_firmware_8801）
//! - 未启用时返回 None，由上层通过 `set_wifi_firmware` 注册后由 `get_firmware_by_name` 的注册表提供

#[cfg(feature = "embed_firmware")]
pub fn get_firmware_by_name(name: &str) -> Option<&'static [u8]> {
    match name {
        "fmacfw.bin" => Some(include_bytes!("../../../firmware/fmacfw.bin")),
        "fmacfw_patch.bin" => Some(include_bytes!("../../../firmware/fmacfw_patch.bin")),
        "fmacfw_rf.bin" => Some(include_bytes!("../../../firmware/fmacfw_rf.bin")),
        "fw_adid.bin" => Some(include_bytes!("../../../firmware/fw_adid.bin")),
        "fw_adid_u03.bin" => Some(include_bytes!("../../../firmware/fw_adid_u03.bin")),
        "fw_patch.bin" => Some(include_bytes!("../../../firmware/fw_patch.bin")),
        "fw_patch_u03.bin" => Some(include_bytes!("../../../firmware/fw_patch_u03.bin")),
        "fw_patch_table.bin" => Some(include_bytes!("../../../firmware/fw_patch_table.bin")),
        "fw_patch_table_u03.bin" => Some(include_bytes!("../../../firmware/fw_patch_table_u03.bin")),
        "fmacfw_patch_8800dc_u02.bin" => Some(include_bytes!("../../../firmware/fmacfw_patch_8800dc_u02.bin")),
        "fw_adid_8800dc_u02.bin" => Some(include_bytes!("../../../firmware/fw_adid_8800dc_u02.bin")),
        "fw_patch_8800dc_u02.bin" => Some(include_bytes!("../../../firmware/fw_patch_8800dc_u02.bin")),
        "fw_patch_table_8800dc_u02.bin" => Some(include_bytes!("../../../firmware/fw_patch_table_8800dc_u02.bin")),
        "fmacfw_8800d80_u02.bin" => Some(include_bytes!("../../../firmware/fmacfw_8800d80_u02.bin")),
        "fw_adid_8800d80_u02.bin" => Some(include_bytes!("../../../firmware/fw_adid_8800d80_u02.bin")),
        "fw_patch_8800d80_u02.bin" => Some(include_bytes!("../../../firmware/fw_patch_8800d80_u02.bin")),
        "fw_patch_table_8800d80_u02.bin" => Some(include_bytes!("../../../firmware/fw_patch_table_8800d80_u02.bin")),
        _ => None,
    }
}

#[cfg(all(feature = "embed_firmware_8801", not(feature = "embed_firmware")))]
pub fn get_firmware_by_name(name: &str) -> Option<&'static [u8]> {
    match name {
        "fmacfw.bin" => Some(include_bytes!("../../../firmware/fmacfw.bin")),
        "fmacfw_patch.bin" => Some(include_bytes!("../../../firmware/fmacfw_patch.bin")),
        _ => None,
    }
}

#[cfg(not(any(feature = "embed_firmware", feature = "embed_firmware_8801")))]
#[inline]
pub fn get_firmware_by_name(_name: &str) -> Option<&'static [u8]> {
    None
}
