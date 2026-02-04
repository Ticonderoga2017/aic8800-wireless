//! 无线控制平面 - 对应 cfg80211_ops
//!
//! 基于 **ieee80211** crate 的 cfg80211 抽象，提供 MVP 子集 **WiphyOps** 与完整 **Cfg80211Ops** 的 re-export。
//! 参考 docs/cfg80211_ops_对照表.md。

extern crate alloc;
use alloc::string::String;
use core::result::Result;

// Re-export 自 ieee80211，与 LicheeRV cfg80211/mac80211 一一对应
pub use ieee80211::{BssInfo, Ifindex, Nl80211Iftype, StationInfo};

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
    fn get_station(&mut self, iface_id: InterfaceId, mac: &[u8; 6]) -> Result<StationInfo, i32>;
    fn set_tx_power(&mut self, iface_id: InterfaceId, power: i32) -> Result<(), i32>;
    fn get_tx_power(&mut self, iface_id: InterfaceId) -> Result<i32, i32>;
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

    fn get_station(&mut self, _iface_id: InterfaceId, _mac: &[u8; 6]) -> Result<StationInfo, i32> {
        Err(-38)
    }

    fn set_tx_power(&mut self, _iface_id: InterfaceId, _power: i32) -> Result<(), i32> {
        Err(-38)
    }

    fn get_tx_power(&mut self, _iface_id: InterfaceId) -> Result<i32, i32> {
        Err(-38)
    }
}
