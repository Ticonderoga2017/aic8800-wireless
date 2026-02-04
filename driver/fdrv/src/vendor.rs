//! Vendor 命令
//! 对应 aic_vendor.c, aic_vendor.h
//!
//! WiFi 厂商特定命令 (nl80211 vendor commands)

/// Vendor OUI - AICSemi
pub const AIC_OUI: [u8; 3] = [0x00, 0xA0, 0xC5];

/// Vendor 子命令
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VendorSubcmd {
    GetVersion = 1,
    SetMacAddr = 2,
    GetMacAddr = 3,
    Scan = 4,
    Connect = 5,
    Disconnect = 6,
    GetRssi = 7,
    GetLinkSpeed = 8,
}
