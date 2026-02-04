//! cfg80211 抽象
//!
//! 对应 Linux net/cfg80211.h：wiphy、cfg80211_ops、scan/connect/key/station/beacon 等。
//! 参考 docs/cfg80211_ops_对照表.md 与 aic8800 rwnx_main.c 中 rwnx_cfg80211_* 实现。

use crate::ieee80211::Channel;
use core::result::Result;

/// 虚拟接口类型（对应 NL80211_IFTYPE_*）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum Nl80211Iftype {
    #[default]
    Unspecified = 0,
    AdHoc = 1,
    Station = 2,
    Ap = 3,
    ApVlan = 4,
    Wds = 5,
    Monitor = 6,
    MeshPoint = 7,
    P2pClient = 8,
    P2pGo = 9,
    P2pDevice = 10,
}

/// 虚拟接口句柄（由驱动分配，对应 net_device 的“逻辑接口”）
pub type Ifindex = u32;

/// 扫描请求（对应 struct cfg80211_scan_request，简化）
#[derive(Debug, Clone)]
pub struct ScanRequest<'a> {
    /// 要扫描的信道（空表示所有支持信道）
    pub channels: &'a [Channel],
    /// 要扫描的 SSID 列表（空表示被动扫描或全 SSID）
    pub ssids: &'a [[u8; 32]],
    /// 各 SSID 有效长度
    pub ssid_lens: &'a [u8],
}

/// 连接参数（对应 struct cfg80211_connect_params / sme）
#[derive(Debug, Clone)]
pub struct ConnectParams<'a> {
    pub bssid: Option<&'a [u8; 6]>,
    pub ssid: &'a [u8],
    pub ssid_len: usize,
    /// 信道（可选，用于指定 BSS 所在信道）
    pub channel: Option<Channel>,
    /// 认证类型等（简化）
    pub auth_type: u32,
}

/// 密钥参数（对应 struct key_params，简化）
#[derive(Debug, Clone)]
pub struct KeyParams<'a> {
    pub cipher: u32,
    pub key: &'a [u8],
    pub seq: Option<&'a [u8]>,
}

/// 密钥状态（get_key 返回）
#[derive(Debug, Clone, Default)]
pub struct KeyStatus {
    pub present: bool,
    pub cipher: u32,
}

/// BSS/扫描结果项（cfg80211_scan_done 上报的每项，或 get_bss 等）
#[derive(Debug, Clone)]
pub struct BssInfo {
    pub bssid: [u8; 6],
    pub freq: u32,
    pub rssi: i32,
    pub ssid: [u8; 32],
    pub ssid_len: u8,
}

/// 站信息（对应 struct station_info，get_station 返回）
#[derive(Debug, Clone, Default)]
pub struct StationInfo {
    pub filled: u64,
    pub rssi: i32,
    pub tx_rate: u32,
    pub rx_rate: u32,
    pub tx_packets: u64,
    pub rx_packets: u64,
}

/// Beacon 数据（对应 struct cfg80211_beacon_data，简化）
#[derive(Debug, Clone)]
pub struct BeaconData<'a> {
    pub head: &'a [u8],
    pub tail: &'a [u8],
    pub beacon_ies: &'a [u8],
    pub proberesp_ies: &'a [u8],
}

/// 信道定义（对应 struct cfg80211_chan_def）
#[derive(Debug, Clone, Copy)]
pub struct ChanDef {
    pub channel: Channel,
    pub width: ChanWidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ChanWidth {
    #[default]
    NoHT = 0,
    TwentyMhz = 1,
    FortyMhz = 2,
    EightyMhz = 3,
    EightyPlus80Mhz = 4,
    OneSixtyMhz = 5,
}

/// wiphy 参数（set_wiphy_params，对应 u32 changed + 各字段）
#[derive(Debug, Clone, Copy, Default)]
pub struct WiphyParams {
    pub rts_threshold: i32,
    pub frag_threshold: i32,
}

/// 发射功率设置类型（对应 enum nl80211_tx_power_setting）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum TxPowerSetting {
    Automatic = 0,
    Limited = 1,
    Fixed = 2,
}

/// mgmt_tx 参数（发送管理帧）
#[derive(Debug, Clone)]
pub struct MgmtTxParams<'a> {
    pub channel: Option<Channel>,
    pub offchan: bool,
    pub wait_ms: u32,
    pub buf: &'a [u8],
}

/// cfg80211_ops — 与 Linux struct cfg80211_ops 一一对应
///
/// 驱动实现本 trait，内部通过 IPC/固件完成扫描、连接、AP、密钥等。
/// 默认实现均返回 Err(-38)(ENOSYS)，可选接口用 default 方法即可。
pub trait Cfg80211Ops {
    // ---------- 接口 ----------
    fn add_virtual_intf(&mut self, name: &str, iftype: Nl80211Iftype) -> Result<Ifindex, i32>;
    fn del_virtual_intf(&mut self, ifindex: Ifindex) -> Result<(), i32>;
    fn change_virtual_intf(&mut self, ifindex: Ifindex, iftype: Nl80211Iftype) -> Result<(), i32>;

    fn start_p2p_device(&mut self, _ifindex: Ifindex) -> Result<(), i32> {
        Err(-38)
    }
    fn stop_p2p_device(&mut self, _ifindex: Ifindex) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- 扫描与连接 ----------
    fn scan(&mut self, ifindex: Ifindex, request: ScanRequest<'_>) -> Result<(), i32>;
    fn connect(&mut self, ifindex: Ifindex, params: ConnectParams<'_>) -> Result<(), i32>;
    fn disconnect(&mut self, ifindex: Ifindex, reason_code: u16) -> Result<(), i32>;

    fn sched_scan_start(&mut self, _ifindex: Ifindex, _request: ScanRequest<'_>) -> Result<(), i32> {
        Err(-38)
    }
    fn sched_scan_stop(&mut self, _ifindex: Ifindex) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- 密钥 ----------
    fn add_key(&mut self, ifindex: Ifindex, key_index: u8, params: KeyParams<'_>) -> Result<(), i32>;
    fn get_key(&mut self, _ifindex: Ifindex, _key_index: u8) -> Result<KeyStatus, i32> {
        Err(-38)
    }
    fn del_key(&mut self, ifindex: Ifindex, key_index: u8) -> Result<(), i32>;
    fn set_default_key(&mut self, ifindex: Ifindex, key_index: u8) -> Result<(), i32>;
    fn set_default_mgmt_key(&mut self, ifindex: Ifindex, key_index: u8) -> Result<(), i32>;

    // ---------- 站管理 ----------
    fn add_station(&mut self, ifindex: Ifindex, mac: &[u8; 6]) -> Result<(), i32>;
    fn del_station(&mut self, ifindex: Ifindex, mac: Option<&[u8; 6]>, reason_code: u16) -> Result<(), i32>;
    fn change_station(&mut self, _ifindex: Ifindex, _mac: &[u8; 6]) -> Result<(), i32> {
        Err(-38)
    }
    fn get_station(&mut self, ifindex: Ifindex, mac: &[u8; 6]) -> Result<StationInfo, i32>;
    fn dump_station(&mut self, _ifindex: Ifindex, _callback: &mut dyn FnMut(&[u8; 6], &StationInfo) -> bool) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- 管理帧与 AP ----------
    fn mgmt_tx(&mut self, ifindex: Ifindex, params: MgmtTxParams<'_>) -> Result<u64, i32>;
    fn start_ap(&mut self, ifindex: Ifindex, beacon: BeaconData<'_>, chandef: ChanDef) -> Result<(), i32>;
    fn change_beacon(&mut self, ifindex: Ifindex, beacon: BeaconData<'_>) -> Result<(), i32>;
    fn stop_ap(&mut self, ifindex: Ifindex) -> Result<(), i32>;
    fn probe_client(&mut self, _ifindex: Ifindex, _peer: &[u8; 6]) -> Result<u64, i32> {
        Err(-38)
    }
    fn set_monitor_channel(&mut self, _chandef: ChanDef) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- 信道与功率 ----------
    fn set_wiphy_params(&mut self, params: WiphyParams) -> Result<(), i32>;
    fn set_txq_params(&mut self, _ifindex: Ifindex, _params: crate::mac80211::TxqParams) -> Result<(), i32> {
        Err(-38)
    }
    fn set_tx_power(&mut self, ifindex: Ifindex, setting: TxPowerSetting, mbm: i32) -> Result<(), i32>;
    fn get_tx_power(&mut self, ifindex: Ifindex) -> Result<i32, i32>;
    fn set_power_mgmt(&mut self, ifindex: Ifindex, enabled: bool, timeout_ms: i32) -> Result<(), i32>;
    fn get_channel(&mut self, ifindex: Ifindex) -> Result<ChanDef, i32>;
    fn remain_on_channel(&mut self, _ifindex: Ifindex, _channel: Channel, _duration_ms: u32) -> Result<u64, i32> {
        Err(-38)
    }
    fn cancel_remain_on_channel(&mut self, _ifindex: Ifindex, _cookie: u64) -> Result<(), i32> {
        Err(-38)
    }
    fn dump_survey(&mut self, _ifindex: Ifindex, _idx: usize) -> Result<SurveyInfo, i32> {
        Err(-38)
    }

    // ---------- DFS / 监管 ----------
    fn start_radar_detection(&mut self, _ifindex: Ifindex, _chandef: ChanDef) -> Result<(), i32> {
        Err(-38)
    }
    fn reg_notifier(&mut self, _alpha2: &[u8; 2]) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- CQM / FT / 信道切换 ----------
    fn update_ft_ies(&mut self, _ifindex: Ifindex, _ies: &[u8]) -> Result<(), i32> {
        Err(-38)
    }
    fn set_cqm_rssi_config(&mut self, _ifindex: Ifindex, _rssi_thold: i32, _rssi_hyst: u32) -> Result<(), i32> {
        Err(-38)
    }
    fn channel_switch(&mut self, _ifindex: Ifindex, _chandef: ChanDef, _count: u32) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- TDLS ----------
    fn tdls_mgmt(&mut self, _ifindex: Ifindex, _peer: &[u8; 6], _buf: &[u8]) -> Result<u64, i32> {
        Err(-38)
    }
    fn tdls_oper(&mut self, _ifindex: Ifindex, _peer: &[u8; 6], _oper: u8) -> Result<(), i32> {
        Err(-38)
    }
    fn tdls_channel_switch(&mut self, _ifindex: Ifindex, _peer: &[u8; 6], _oper_class: u8) -> Result<(), i32> {
        Err(-38)
    }
    fn tdls_cancel_channel_switch(&mut self, _ifindex: Ifindex, _peer: &[u8; 6]) -> Result<(), i32> {
        Err(-38)
    }

    // ---------- 其它 ----------
    fn change_bss(&mut self, _ifindex: Ifindex, _params: BssParams) -> Result<(), i32> {
        Err(-38)
    }
    fn external_auth(&mut self, _ifindex: Ifindex, _params: ExternalAuthParams<'_>) -> Result<(), i32> {
        Err(-38)
    }
}

/// BSS 参数（change_bss）
#[derive(Debug, Clone, Default)]
pub struct BssParams {
    pub use_cts_prot: Option<bool>,
    pub use_short_preamble: Option<bool>,
    pub use_short_slot: Option<bool>,
}

/// 外部认证参数（WPA3 SAE 等）
#[derive(Debug, Clone)]
pub struct ExternalAuthParams<'a> {
    pub bssid: [u8; 6],
    pub ssid: &'a [u8],
    pub action: u8,
}

/// 信道 survey 信息（dump_survey）
#[derive(Debug, Clone, Default)]
pub struct SurveyInfo {
    pub filled: u64,
    pub channel: Option<Channel>,
    pub noise: i8,
    pub time_busy: u64,
    pub time_rx: u64,
    pub time_tx: u64,
}
