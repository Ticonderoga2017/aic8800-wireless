//! mac80211 抽象
//!
//! 对应 Linux net/mac80211.h：ieee80211_hw、ieee80211_conf、supported_band、txq_params 等。
//! aic8800 使用 ieee80211_scan_completed、ieee80211_supported_band、ieee80211_txq_params 等。

use crate::ieee80211::{Band, Channel, Rate};

/// 软件 TX 队列参数（对应 struct ieee80211_txq_params）
#[derive(Debug, Clone, Copy, Default)]
pub struct TxqParams {
    pub ac: u8,
    pub txop: u16,
    pub cw_min: u16,
    pub cw_max: u16,
    pub aifs: u8,
}

/// 支持的频段（对应 struct ieee80211_supported_band）
#[derive(Debug, Clone)]
pub struct SupportedBand {
    pub band: Band,
    pub channels: &'static [Channel],
    pub rates: &'static [Rate],
    pub n_channels: usize,
    pub n_bitrates: usize,
    pub ht_cap: Option<HtCap>,
    pub vht_cap: Option<VhtCap>,
}

/// HT 能力（简化，对应 ieee80211_sta_ht_cap）
#[derive(Debug, Clone, Copy, Default)]
pub struct HtCap {
    pub cap: u16,
    pub ampdu_factor: u8,
    pub ampdu_density: u8,
    pub mcs: [u8; 16],
}

/// VHT 能力（简化）
#[derive(Debug, Clone, Copy, Default)]
pub struct VhtCap {
    pub cap: u32,
    pub vht_mcs: [u16; 2],
}

/// 硬件配置（对应 struct ieee80211_conf）
#[derive(Debug, Clone, Copy, Default)]
pub struct Conf {
    pub channel: Option<Channel>,
    pub listen_interval: u16,
    pub power_level: i8,
    pub flags: u32,
}

/// 硬件抽象（对应 struct ieee80211_hw）
///
/// 驱动实现此 trait，表示“一块 WiFi 硬件”。与 cfg80211 的 wiphy 对应：
/// Linux 中 wiphy 与 ieee80211_hw 一一绑定，rwnx_hw 同时持有两者。
pub trait Hw {
    /// 当前配置（信道、省电等）
    fn conf(&self) -> Conf;

    /// 更新配置（由 set_channel、set_power 等间接调用）
    fn set_conf(&mut self, conf: &Conf) -> Result<(), i32>;

    /// 支持的 2.4G 频段（含信道与速率表）
    fn band_2ghz(&self) -> Option<&SupportedBand>;

    /// 支持的 5G 频段
    fn band_5ghz(&self) -> Option<&SupportedBand>;

    /// 扫描完成上报（对应 ieee80211_scan_completed）
    fn report_scan_completed(&mut self, aborted: bool) -> Result<(), i32>;

    /// 信道切换通知（对应 ieee80211_channel_switch_disabled）
    fn report_channel_switch(&mut self, _chandef: crate::cfg80211::ChanDef) -> Result<(), i32> {
        Ok(())
    }
}
