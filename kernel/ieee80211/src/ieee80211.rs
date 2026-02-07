//! IEEE 802.11 类型与常量
//!
//! 对应 Linux include/linux/ieee80211.h、net/ieee80211_radiotap.h 中 aic8800 用到的部分。

/// 频段（对应 NL80211_BAND_*）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Band {
    #[default]
    TwoGhz = 0,
    FiveGhz = 1,
    SixGhz = 2,
}

/// 信道（对应 struct ieee80211_channel）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    /// 中心频率 MHz
    pub center_freq: u32,
    /// 频段
    pub band: Band,
    /// 最大功率 0.25 dBm 单位（如 30*4 = 120 表示 30 dBm）
    pub max_power: i8,
}

impl Channel {
    pub const fn new_2g(freq_mhz: u32, max_power_dbm: i8) -> Self {
        Self {
            center_freq: freq_mhz,
            band: Band::TwoGhz,
            max_power: max_power_dbm,
        }
    }

    pub const fn new_5g(freq_mhz: u32, max_power_dbm: i8) -> Self {
        Self {
            center_freq: freq_mhz,
            band: Band::FiveGhz,
            max_power: max_power_dbm,
        }
    }
}

/// 速率（对应 struct ieee80211_rate）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    /// 标称速率 100 kbps（如 10 表示 1 Mbps）
    pub bitrate_100kbps: u16,
    /// 硬件速率索引
    pub hw_value: u16,
    /// IEEE80211_RATE_* 标志
    pub flags: u16,
}

/// 速率标志（Linux IEEE80211_RATE_*）
pub mod rate_flags {
    pub const SHORT_PREAMBLE: u16 = 1 << 0;
    pub const BASIC: u16 = 1 << 1;
}

/// 802.11 信息元素 ID（WLAN_EID_*）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WlanEid {
    Ssid = 0,
    SupportedRates = 1,
    DsParams = 3,
    HtCapability = 45,
    VhtCapability = 191,
    Extension = 255,
}

impl WlanEid {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// 管理帧类型（frame_control 子域，对应 ieee80211_is_*）
pub mod fc {
    pub const TYPE_MGMT: u16 = 0x0000;
    pub const TYPE_CTL: u16 = 0x0004;
    pub const TYPE_DATA: u16 = 0x0008;
    pub const SUBTYPE_BEACON: u16 = 0x0080;
    pub const SUBTYPE_ASSOC_REQ: u16 = 0x0000;
    pub const SUBTYPE_ASSOC_RESP: u16 = 0x0010;
    pub const SUBTYPE_REASSOC_REQ: u16 = 0x0020;
    pub const SUBTYPE_AUTH: u16 = 0x00B0;
    pub const SUBTYPE_DEAUTH: u16 = 0x00C0;
    pub const SUBTYPE_DISASSOC: u16 = 0x00A0;
    pub const SUBTYPE_PROBE_REQ: u16 = 0x0040;
    pub const SUBTYPE_PROBE_RESP: u16 = 0x0050;
    pub const SUBTYPE_ACTION: u16 = 0x00D0;
}

/// 从帧取 frame_control（前 2 字节，little-endian）
#[inline]
pub fn frame_control(buf: &[u8]) -> u16 {
    if buf.len() >= 2 {
        u16::from_le_bytes([buf[0], buf[1]])
    } else {
        0
    }
}

/// 是否管理帧
#[inline]
pub fn is_mgmt(fc: u16) -> bool {
    (fc & 0x000C) == fc::TYPE_MGMT
}

/// 是否 Beacon
#[inline]
pub fn is_beacon(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_BEACON
}

/// 是否 Assoc Request
#[inline]
pub fn is_assoc_req(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_ASSOC_REQ
}

/// 是否 Reassoc Request
#[inline]
pub fn is_reassoc_req(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_REASSOC_REQ
}

/// 是否 Auth
#[inline]
pub fn is_auth(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_AUTH
}

/// 是否 Deauth / Disassoc
#[inline]
pub fn is_deauth(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_DEAUTH
}

#[inline]
pub fn is_disassoc(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_DISASSOC
}

/// 是否 Probe Request
#[inline]
pub fn is_probe_req(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_PROBE_REQ
}

/// 是否 Action
#[inline]
pub fn is_action(fc: u16) -> bool {
    is_mgmt(fc) && (fc & 0x00F0) == fc::SUBTYPE_ACTION
}

// ========== 与 LicheeRV / Linux 对齐的常量 ==========
// 对应 include/uapi/linux/nl80211.h WLAN_CIPHER_SUITE_*
pub mod wlan_cipher_suite {
    pub const WEP40: u32 = 0x000F_AC01;
    pub const WEP104: u32 = 0x000F_AC05;
    pub const TKIP: u32 = 0x000F_AC02;
    pub const CCMP: u32 = 0x000F_AC04;
    pub const AES_CMAC: u32 = 0x000F_AC06;
    pub const GCMP: u32 = 0x000F_AC08;
    pub const GCMP_256: u32 = 0x000F_AC09;
    pub const CCMP_256: u32 = 0x000F_AC10;
    pub const BIP_GMAC_128: u32 = 0x000F_AC0B;
    pub const BIP_GMAC_256: u32 = 0x000F_AC0C;
    pub const BIP_CMAC_256: u32 = 0x000F_AC0D;
}

/// 对应 lmac_mac.h enum mac_cipher_suite，与 rwnx_cfg80211_add_key 中 switch(params->cipher) 一致
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MacCipherSuite {
    Wep40 = 0,
    Tkip = 1,
    Ccmp = 2,
    Wep104 = 3,
    WpiSms4 = 4,
    BipCmac128 = 5,
    Gcmp128 = 6,
    Gcmp256 = 7,
    Ccmp256 = 8,
    BipGmac128 = 9,
    BipGmac256 = 10,
    BipCmac256 = 11,
    Invalid = 0xFF,
}

/// WLAN_CIPHER_SUITE_* -> MAC_CIPHER_*，与 rwnx_main.c rwnx_cfg80211_add_key 一致
pub fn wlan_cipher_to_mac(cipher: u32) -> Option<MacCipherSuite> {
    use wlan_cipher_suite::*;
    match cipher {
        WEP40 => Some(MacCipherSuite::Wep40),
        WEP104 => Some(MacCipherSuite::Wep104),
        TKIP => Some(MacCipherSuite::Tkip),
        CCMP => Some(MacCipherSuite::Ccmp),
        AES_CMAC => Some(MacCipherSuite::BipCmac128),
        GCMP => Some(MacCipherSuite::Gcmp128),
        GCMP_256 => Some(MacCipherSuite::Gcmp256),
        CCMP_256 => Some(MacCipherSuite::Ccmp256),
        BIP_GMAC_128 => Some(MacCipherSuite::BipGmac128),
        BIP_GMAC_256 => Some(MacCipherSuite::BipGmac256),
        BIP_CMAC_256 => Some(MacCipherSuite::BipCmac256),
        _ => None,
    }
}

/// MAC 密钥最大长度，与 lmac_mac.h MAC_SEC_KEY_LEN 一致
pub const MAC_SEC_KEY_LEN: usize = 32;
