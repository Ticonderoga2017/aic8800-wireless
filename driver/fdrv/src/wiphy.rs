//! 无线控制平面 - 对应 cfg80211_ops
//!
//! 基于 **ieee80211** crate 的 cfg80211 抽象，提供 MVP 子集 **WiphyOps** 与完整 **Cfg80211Ops** 的 re-export。
//! 参考 docs/cfg80211_ops_对照表.md。

extern crate alloc;
use alloc::string::String;
use core::result::Result;

// Re-export 自 ieee80211，与 LicheeRV cfg80211/mac80211 一一对应
pub use ieee80211::{BssInfo, Ifindex, KeyStatus, Nl80211Iftype, StationInfo};

/// 虚拟接口类型（NL80211_IFTYPE_*）— 与 ieee80211::Nl80211Iftype 同义
pub type IfaceType = Nl80211Iftype;

/// 接口句柄（由驱动分配）— 与 ieee80211::Ifindex 同义
pub type InterfaceId = Ifindex;

/// 扫描结果项（带 String SSID，便于上层使用；可从 ieee80211::BssInfo 转换）
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub freq: u32,
    pub rssi: i32,
}

impl ScanResult {
    pub fn from_bss_info(b: &BssInfo) -> Self {
        let ssid_len = b.ssid_len.min(32) as usize;
        let ssid = alloc::string::String::from(
            core::str::from_utf8(&b.ssid[..ssid_len]).unwrap_or(""),
        );
        Self {
            ssid,
            bssid: b.bssid,
            freq: b.freq,
            rssi: b.rssi,
        }
    }
}

/// 无线控制平面操作 - 对应 cfg80211_ops 的 MVP 子集
///
/// 由 fdrv 实现，内部通过 IPC 与固件通信。
/// 完整 ops 见 **Cfg80211Ops**（ieee80211 crate）。
pub trait WiphyOps {
    fn add_interface(&mut self, iface_type: IfaceType) -> Result<InterfaceId, i32>;
    fn del_interface(&mut self, iface_id: InterfaceId) -> Result<(), i32>;
    fn scan(&mut self, iface_id: InterfaceId) -> Result<(), i32>;
    fn connect(&mut self, iface_id: InterfaceId, ssid: &[u8], bssid: Option<&[u8; 6]>) -> Result<(), i32>;
    fn disconnect(&mut self, iface_id: InterfaceId) -> Result<(), i32>;
    fn start_ap(&mut self, iface_id: InterfaceId, ssid: &[u8], channel: u8) -> Result<(), i32>;
    fn stop_ap(&mut self, iface_id: InterfaceId) -> Result<(), i32>;
    fn add_key(&mut self, iface_id: InterfaceId, key_index: u8, key_data: &[u8]) -> Result<(), i32>;
    fn del_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32>;
    /// 设置默认数据密钥（与 LicheeRV 一致：仅保存索引，不调 LMAC）
    fn set_default_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32>;
    /// 查询密钥状态（与 LicheeRV get_key 一致；无 MM_GET_KEY 时从 add_key 状态返回）
    fn get_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<KeyStatus, i32>;
    /// 设置默认管理帧密钥（与 LicheeRV 一致：仅保存，不调 LMAC）
    fn set_default_mgmt_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32>;
    fn get_station(&mut self, iface_id: InterfaceId, mac: &[u8; 6]) -> Result<StationInfo, i32>;
    /// AP 模式添加关联站（与 LicheeRV add_station 一致；MM_STA_ADD_REQ）
    fn add_station(&mut self, iface_id: InterfaceId, mac: &[u8; 6]) -> Result<(), i32>;
    /// AP 模式删除站（与 LicheeRV del_station 一致；MM_STA_DEL_REQ，sta_idx）
    fn del_station(&mut self, iface_id: InterfaceId, sta_idx: u8) -> Result<(), i32>;
    fn set_tx_power(&mut self, iface_id: InterfaceId, power: i32) -> Result<(), i32>;
    fn get_tx_power(&mut self, iface_id: InterfaceId) -> Result<i32, i32>;
    /// 当前信道（与 LicheeRV get_channel 一致；无信道上下文时返回 -ENODATA）
    fn get_channel(&mut self, iface_id: InterfaceId) -> Result<u8, i32>;
    /// 省电模式（与 LicheeRV set_power_mgmt 一致；可选 MM_SET_PS_MODE）
    fn set_power_mgmt(&mut self, iface_id: InterfaceId, enabled: bool) -> Result<(), i32>;
    /// wiphy 参数（RTS/分片阈值等）；可选，未实现返回 -ENOSYS
    fn set_wiphy_params(&mut self, _rts_threshold: Option<i32>, _frag_threshold: Option<i32>) -> Result<(), i32>;
}

/// 默认实现：所有操作返回“未实现”，用于占位与测试
#[derive(Debug, Default)]
pub struct WiphyOpsStub;

impl WiphyOps for WiphyOpsStub {
    fn add_interface(&mut self, iface_type: IfaceType) -> Result<InterfaceId, i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub add_interface type={:?}", iface_type);
        Ok(0)
    }

    fn del_interface(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub del_interface id={}", iface_id);
        Ok(())
    }

    fn scan(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub scan iface_id={} (unimplemented)", iface_id);
        Err(-38)
    }

    fn connect(
        &mut self,
        iface_id: InterfaceId,
        _ssid: &[u8],
        _bssid: Option<&[u8; 6]>,
    ) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub connect iface_id={} (unimplemented)", iface_id);
        Err(-38)
    }

    fn disconnect(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub disconnect iface_id={}", iface_id);
        Ok(())
    }

    fn start_ap(&mut self, iface_id: InterfaceId, _ssid: &[u8], _channel: u8) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub start_ap iface_id={} (unimplemented)", iface_id);
        Err(-38)
    }

    fn stop_ap(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        log::debug!(target: "wireless::fdrv", "WiphyOpsStub stop_ap iface_id={}", iface_id);
        Ok(())
    }

    fn add_key(&mut self, _iface_id: InterfaceId, _key_index: u8, _key_data: &[u8]) -> Result<(), i32> {
        Err(-38)
    }

    fn del_key(&mut self, _iface_id: InterfaceId, _key_index: u8) -> Result<(), i32> {
        Err(-38)
    }

    fn set_default_key(&mut self, _iface_id: InterfaceId, _key_index: u8) -> Result<(), i32> {
        Ok(())
    }

    fn get_key(&mut self, _iface_id: InterfaceId, _key_index: u8) -> Result<KeyStatus, i32> {
        Err(-38)
    }

    fn set_default_mgmt_key(&mut self, _iface_id: InterfaceId, _key_index: u8) -> Result<(), i32> {
        Ok(())
    }

    fn get_station(&mut self, _iface_id: InterfaceId, _mac: &[u8; 6]) -> Result<StationInfo, i32> {
        Err(-38)
    }

    fn add_station(&mut self, _iface_id: InterfaceId, _mac: &[u8; 6]) -> Result<(), i32> {
        Err(-38)
    }

    fn del_station(&mut self, _iface_id: InterfaceId, _sta_idx: u8) -> Result<(), i32> {
        Err(-38)
    }

    fn set_tx_power(&mut self, _iface_id: InterfaceId, _power: i32) -> Result<(), i32> {
        Err(-38)
    }

    fn get_tx_power(&mut self, _iface_id: InterfaceId) -> Result<i32, i32> {
        Err(-38)
    }

    fn get_channel(&mut self, _iface_id: InterfaceId) -> Result<u8, i32> {
        Err(-61)
    }

    fn set_power_mgmt(&mut self, _iface_id: InterfaceId, _enabled: bool) -> Result<(), i32> {
        Ok(())
    }

    fn set_wiphy_params(&mut self, _rts: Option<i32>, _frag: Option<i32>) -> Result<(), i32> {
        Err(-38)
    }
}
