//! WiphyOps 真实实现：通过 BSP IPC 发送 LMAC 命令，与 aic8800 rwnx_main.c cfg80211_ops 对齐
//! 含 scan/connect/disconnect、start_ap/stop_ap、add_key/del_key/set_default_key、get_station

use bsp::{
    aicbsp_current_product_id, submit_cmd_tx_and_wait_tx_done, with_cmd_mgr, sdio_poll_rx_once,
    LmacMsg, ProductId, RwnxCmdMgr,
    SCANU_START_CFM, SM_CONNECT_CFM, SM_DISCONNECT_CFM,
    MM_ADD_IF_CFM, MM_REMOVE_IF_CFM, MM_STA_ADD_CFM, MM_STA_DEL_CFM,
    MM_KEY_ADD_CFM, MM_KEY_DEL_CFM, MM_GET_STA_INFO_CFM, MM_SET_POWER_CFM,
    APM_START_CFM, APM_STOP_CFM,
    RWNX_80211_CMD_TIMEOUT_MS,
};
use core::result::Result;

use crate::lmac_cmd::{
    build_mm_sta_add_req, build_mm_sta_del_req, build_scanu_start_req, build_sm_connect_req,
    build_sm_disconnect_req, build_mm_add_if_req, build_mm_remove_if_req,
    build_mm_key_add_req, build_mm_key_del_req, build_mm_get_sta_info_req,
    build_apm_start_req, build_apm_stop_req, build_mm_set_power_req,
    parse_mm_add_if_cfm, parse_mm_key_add_cfm, parse_mm_get_sta_info_cfm, parse_apm_start_cfm,
    parse_sm_connect_cfm, parse_mm_set_power_cfm, parse_mm_sta_add_cfm, parse_mm_sta_del_cfm,
    MacVifType,
};
use ieee80211::{KeyStatus, StationInfo, wlan_cipher_to_mac, nl80211_sta_info};
use crate::wiphy::{InterfaceId, IfaceType, WiphyOps};

/// 最大 VIF 数
const MAX_VIF: usize = 4;
/// 每 VIF 最大密钥槽位
const MAX_KEYS_PER_VIF: usize = 8;
/// STA 表项数（mac -> sta_idx）
const STA_TABLE_LEN: usize = 16;

/// 单条 STA 表项
#[derive(Clone, Copy, Default)]
struct StaEntry {
    mac: [u8; 6],
    sta_idx: u8,
    used: bool,
}

/// 内部状态：密钥 hw_key_idx、默认密钥、STA 表、上次设置的 TX 功率、当前信道、默认 mgmt 密钥
struct WiphyState {
    /// key_hw[vif_id][key_index] = hw_key_idx from MM_KEY_ADD_CFM
    key_hw: [[Option<u8>; MAX_KEYS_PER_VIF]; MAX_VIF],
    /// default_key_index per vif（set_default_key 仅保存，不调 LMAC）
    default_key: [Option<u8>; MAX_VIF],
    /// set_default_mgmt_key 仅保存（与 LicheeRV 一致）
    default_mgmt_key: [Option<u8>; MAX_VIF],
    sta_table: [StaEntry; STA_TABLE_LEN],
    last_tx_power_dbm: [Option<i8>; MAX_VIF],
    /// start_ap/connect 后保存，get_channel 返回
    current_channel: [Option<u8>; MAX_VIF],
}

impl Default for WiphyState {
    fn default() -> Self {
        Self {
            key_hw: [[None; MAX_KEYS_PER_VIF]; MAX_VIF],
            default_key: [None; MAX_VIF],
            default_mgmt_key: [None; MAX_VIF],
            sta_table: [StaEntry::default(); STA_TABLE_LEN],
            last_tx_power_dbm: [None; MAX_VIF],
            current_channel: [None; MAX_VIF],
        }
    }
}

/// 8801 IPC 发送长度：与 BSP flow::ipc_send_len_8801 一致
fn ipc_send_len_8801(serialized_len: usize) -> usize {
    const TX_ALIGNMENT: usize = 4;
    const BLOCK: usize = 512;
    const TAIL_LEN: usize = 4;
    let len4 = (serialized_len + TX_ALIGNMENT - 1) / TX_ALIGNMENT * TX_ALIGNMENT;
    if len4 % BLOCK == 0 {
        len4
    } else {
        ((len4 + TAIL_LEN + BLOCK - 1) / BLOCK) * BLOCK
    }
}

fn send_lmac_cmd_and_wait_cfm(
    msg: &LmacMsg,
    cfm_id: u16,
    timeout_ms: u32,
) -> Result<(), i32> {
    send_lmac_cmd_and_wait_cfm_with_buf(msg, cfm_id, timeout_ms, &mut [0u8; 64]).map(|_| ())
}

/// 发送 REQ 并等待 CFM，将 CFM 的 param 写入 cfm_buf，返回写入长度
fn send_lmac_cmd_and_wait_cfm_with_buf(
    msg: &LmacMsg,
    cfm_id: u16,
    timeout_ms: u32,
    cfm_buf: &mut [u8],
) -> Result<usize, i32> {
    let product_id = aicbsp_current_product_id().ok_or(-22)?;
    let mut buf = [0u8; 512];
    let len = if product_id == ProductId::Aic8801 {
        msg.serialize_8801(&mut buf)
    } else {
        msg.serialize(&mut buf)
    };
    let send_len = if product_id == ProductId::Aic8801 {
        ipc_send_len_8801(len)
    } else {
        len
    };
    if send_len > buf.len() {
        return Err(-22);
    }
    if send_len > len {
        buf[len..send_len].fill(0);
    }
    let token = with_cmd_mgr(|c| c.push(cfm_id)).flatten().ok_or(-12)?;
    submit_cmd_tx_and_wait_tx_done(&buf[..send_len], send_len)?;
    let mut poll = || sdio_poll_rx_once();
    RwnxCmdMgr::wait_done_until(
        timeout_ms,
        || with_cmd_mgr(|c| c.is_done(token)).unwrap_or(false),
        None,
        Some(&mut poll),
        None,
    )?;
    let n = with_cmd_mgr(|c| c.take_cfm(token, cfm_buf)).flatten().ok_or(-5)?;
    Ok(n)
}

/// WiphyOps 真实实现：基于 BSP IPC 与 lmac_cmd 构建/解析，带 key/sta 状态
pub struct WiphyOpsImpl {
    state: WiphyState,
}

impl WiphyOpsImpl {
    pub fn new() -> Self {
        Self {
            state: WiphyState::default(),
        }
    }

    /// 收到 SM_CONNECT_IND 时调用，登记 STA 模式下当前 AP 的 (bssid, ap_idx)，供 get_station 使用
    pub fn register_sta_from_connect_ind(&mut self, _vif_idx: u8, bssid: &[u8; 6], ap_idx: u8) {
        for e in self.state.sta_table.iter_mut() {
            if !e.used {
                e.mac = *bssid;
                e.sta_idx = ap_idx;
                e.used = true;
                return;
            }
            if e.mac == *bssid {
                e.sta_idx = ap_idx;
                return;
            }
        }
    }

    /// 清除某 VIF 的 STA 登记（disconnect 时可选调用）
    pub fn unregister_sta_by_mac(&mut self, mac: &[u8; 6]) {
        for e in self.state.sta_table.iter_mut() {
            if e.used && e.mac == *mac {
                e.used = false;
                break;
            }
        }
    }

    fn lookup_sta_idx(&self, mac: &[u8; 6]) -> Option<u8> {
        for e in &self.state.sta_table {
            if e.used && e.mac == *mac {
                return Some(e.sta_idx);
            }
        }
        None
    }

    fn fill_station_info_from_cfm(cfm: &crate::lmac_cmd::MmGetStaInfoCfm) -> StationInfo {
        use ieee80211::StationInfo;
        let mut info = StationInfo::default();
        info.filled = nl80211_sta_info::TX_BITRATE | nl80211_sta_info::TX_FAILED | nl80211_sta_info::SIGNAL;
        info.tx_failed = cfm.txfailed;
        info.rssi = cfm.rssi as i32;
        info.tx_rate = cfm.rate_info;
        info.rx_rate = cfm.rate_info;
        info
    }
}

impl WiphyOps for WiphyOpsImpl {
    fn add_interface(&mut self, iface_type: IfaceType) -> Result<InterfaceId, i32> {
        let vif_type = match iface_type {
            IfaceType::Station => MacVifType::Sta,
            IfaceType::Ap => MacVifType::Ap,
            IfaceType::AdHoc => MacVifType::Ibss,
            IfaceType::Monitor => MacVifType::Monitor,
            _ => MacVifType::Sta,
        };
        let mac = [0u8; 6];
        let msg = build_mm_add_if_req(vif_type, &mac, false);
        let mut cfm_buf = [0u8; 8];
        send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_ADD_IF_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let cfm = parse_mm_add_if_cfm(&cfm_buf).ok_or(-5)?;
        if cfm.status != 0 {
            return Err(-5);
        }
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl add_interface type={:?} => inst_nbr={}", iface_type, cfm.inst_nbr);
        Ok(cfm.inst_nbr as InterfaceId)
    }

    fn del_interface(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        let msg = build_mm_remove_if_req(iface_id as u8);
        send_lmac_cmd_and_wait_cfm(&msg, MM_REMOVE_IF_CFM, RWNX_80211_CMD_TIMEOUT_MS)?;
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl del_interface id={}", iface_id);
        Ok(())
    }

    fn scan(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        let msg = build_scanu_start_req(iface_id as u8, 0).ok_or(-22)?;
        send_lmac_cmd_and_wait_cfm(&msg, SCANU_START_CFM, RWNX_80211_CMD_TIMEOUT_MS)?;
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl scan iface_id={} started, results via SCANU_RESULT_IND", iface_id);
        Ok(())
    }

    fn connect(
        &mut self,
        iface_id: InterfaceId,
        ssid: &[u8],
        bssid: Option<&[u8; 6]>,
    ) -> Result<(), i32> {
        let msg = build_sm_connect_req(iface_id as u8, ssid, bssid, None).ok_or(-22)?;
        let mut cfm_buf = [0u8; 8];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, SM_CONNECT_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let status = parse_sm_connect_cfm(&cfm_buf[..n]).unwrap_or(0xff);
        // 与 LicheeRV rwnx_cfg80211_connect 一致：CO_OK=0 -> Ok; CO_BUSY=8 -> -EINPROGRESS; CO_OP_IN_PROGRESS=9 -> -EALREADY; 其它 -> -EIO
        match status {
            0 => {
                log::info!(target: "wireless::fdrv", "WiphyOpsImpl connect iface_id={} ssid_len={}, result via SM_CONNECT_IND", iface_id, ssid.len());
                Ok(())
            }
            8 => Err(-115),  // CO_BUSY -> EINPROGRESS
            9 => Err(-114),  // CO_OP_IN_PROGRESS -> EALREADY
            _ => Err(-5),    // EIO
        }
    }

    fn disconnect(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        let msg = build_sm_disconnect_req(iface_id as u8, 3);
        send_lmac_cmd_and_wait_cfm(&msg, SM_DISCONNECT_CFM, RWNX_80211_CMD_TIMEOUT_MS)?;
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl disconnect iface_id={}", iface_id);
        Ok(())
    }

    fn start_ap(&mut self, iface_id: InterfaceId, _ssid: &[u8], channel: u8) -> Result<(), i32> {
        let vif_idx = iface_id as usize;
        let basic_rates: [u8; 4] = [0x82, 0x84, 0x8b, 0x96]; // 1,2,5.5,11 Mbps basic
        let msg = build_apm_start_req(iface_id as u8, channel, 100, &basic_rates).ok_or(-22)?;
        let mut cfm_buf = [0u8; 32];
        send_lmac_cmd_and_wait_cfm_with_buf(&msg, APM_START_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let cfm = parse_apm_start_cfm(&cfm_buf).ok_or(-5)?;
        if cfm.status != 0 {
            log::warn!(target: "wireless::fdrv", "WiphyOpsImpl start_ap APM_START_CFM status={}", cfm.status);
            return Err(-5);
        }
        if cfm.bcmc_idx < STA_TABLE_LEN as u8 {
            let e = &mut self.state.sta_table[cfm.bcmc_idx as usize];
            e.mac = [0xff; 6];
            e.sta_idx = cfm.bcmc_idx;
            e.used = true;
        }
        if vif_idx < MAX_VIF {
            self.state.current_channel[vif_idx] = Some(channel);
        }
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl start_ap iface_id={} ch={} bcmc_idx={}", iface_id, channel, cfm.bcmc_idx);
        Ok(())
    }

    fn stop_ap(&mut self, iface_id: InterfaceId) -> Result<(), i32> {
        let msg = build_apm_stop_req(iface_id as u8);
        send_lmac_cmd_and_wait_cfm(&msg, APM_STOP_CFM, RWNX_80211_CMD_TIMEOUT_MS)?;
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl stop_ap iface_id={}", iface_id);
        Ok(())
    }

    fn add_key(&mut self, iface_id: InterfaceId, key_index: u8, key_data: &[u8]) -> Result<(), i32> {
        let vif_idx = iface_id as u8;
        let cipher = ieee80211::wlan_cipher_suite::CCMP;
        let mac_cipher = wlan_cipher_to_mac(cipher).ok_or(-22)?;
        let sta_idx = 0xFF;
        let msg = build_mm_key_add_req(vif_idx, key_index, sta_idx, key_data, mac_cipher, false)
            .ok_or(-22)?;
        let mut cfm_buf = [0u8; 8];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_KEY_ADD_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let cfm = parse_mm_key_add_cfm(&cfm_buf[..n]).ok_or(-5)?;
        if cfm.status != 0 {
            return Err(-5);
        }
        if (vif_idx as usize) < MAX_VIF && (key_index as usize) < MAX_KEYS_PER_VIF {
            self.state.key_hw[vif_idx as usize][key_index as usize] = Some(cfm.hw_key_idx);
        }
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl add_key iface_id={} idx={} hw_key_idx={}", iface_id, key_index, cfm.hw_key_idx);
        Ok(())
    }

    fn del_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32> {
        let vif_idx = iface_id as usize;
        let hw_key_idx = if vif_idx < MAX_VIF && (key_index as usize) < MAX_KEYS_PER_VIF {
            self.state.key_hw[vif_idx][key_index as usize].ok_or(-2)?
        } else {
            return Err(-2);
        };
        let msg = build_mm_key_del_req(hw_key_idx);
        send_lmac_cmd_and_wait_cfm(&msg, MM_KEY_DEL_CFM, RWNX_80211_CMD_TIMEOUT_MS)?;
        self.state.key_hw[vif_idx][key_index as usize] = None;
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl del_key iface_id={} idx={} hw_key_idx={}", iface_id, key_index, hw_key_idx);
        Ok(())
    }

    fn set_default_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32> {
        if (iface_id as usize) < MAX_VIF {
            self.state.default_key[iface_id as usize] = Some(key_index);
        }
        Ok(())
    }

    fn get_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<KeyStatus, i32> {
        let vif_idx = iface_id as usize;
        if vif_idx >= MAX_VIF || (key_index as usize) >= MAX_KEYS_PER_VIF {
            return Err(-22);
        }
        let present = self.state.key_hw[vif_idx][key_index as usize].is_some();
        Ok(KeyStatus {
            present,
            cipher: 0,
        })
    }

    fn set_default_mgmt_key(&mut self, iface_id: InterfaceId, key_index: u8) -> Result<(), i32> {
        let vif_idx = iface_id as usize;
        if vif_idx >= MAX_VIF {
            return Err(-22);
        }
        self.state.default_mgmt_key[vif_idx] = Some(key_index);
        Ok(())
    }

    fn get_station(&mut self, _iface_id: InterfaceId, mac: &[u8; 6]) -> Result<StationInfo, i32> {
        let sta_idx = self.lookup_sta_idx(mac).ok_or(-2)?;
        let msg = build_mm_get_sta_info_req(sta_idx);
        let mut cfm_buf = [0u8; 64];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_GET_STA_INFO_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let cfm = parse_mm_get_sta_info_cfm(&cfm_buf[..n]).ok_or(-5)?;
        Ok(Self::fill_station_info_from_cfm(&cfm))
    }

    fn add_station(&mut self, iface_id: InterfaceId, mac: &[u8; 6]) -> Result<(), i32> {
        let msg = build_mm_sta_add_req(iface_id as u8, mac);
        let mut cfm_buf = [0u8; 8];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_STA_ADD_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let status = parse_mm_sta_add_cfm(&cfm_buf[..n]).unwrap_or(0xff);
        if status != 0 {
            return Err(-5);
        }
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl add_station iface_id={} mac={:02x?}", iface_id, mac);
        Ok(())
    }

    fn del_station(&mut self, _iface_id: InterfaceId, sta_idx: u8) -> Result<(), i32> {
        let msg = build_mm_sta_del_req(sta_idx);
        let mut cfm_buf = [0u8; 8];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_STA_DEL_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let status = parse_mm_sta_del_cfm(&cfm_buf[..n]).unwrap_or(0xff);
        if status != 0 {
            return Err(-5);
        }
        Ok(())
    }

    fn set_tx_power(&mut self, iface_id: InterfaceId, power: i32) -> Result<(), i32> {
        let vif_idx = iface_id as usize;
        if vif_idx >= MAX_VIF {
            return Err(-22);
        }
        // 与 LicheeRV rwnx_cfg80211_set_tx_power 一致：power 为 mBm，转 dBm；NL80211_TX_POWER_AUTOMATIC 用 0x7f
        let power_dbm = if power == i32::MAX || power >= 12700 {
            0x7fi8
        } else {
            (power / 100) as i8
        };
        let msg = build_mm_set_power_req(iface_id as u8, power_dbm);
        let mut cfm_buf = [0u8; 8];
        let n = send_lmac_cmd_and_wait_cfm_with_buf(&msg, MM_SET_POWER_CFM, RWNX_80211_CMD_TIMEOUT_MS, &mut cfm_buf)?;
        let _cfm = parse_mm_set_power_cfm(&cfm_buf[..n]).ok_or(-5)?;
        self.state.last_tx_power_dbm[vif_idx] = Some(power_dbm);
        log::info!(target: "wireless::fdrv", "WiphyOpsImpl set_tx_power iface_id={} power_dbm={}", iface_id, power_dbm);
        Ok(())
    }

    fn get_tx_power(&mut self, iface_id: InterfaceId) -> Result<i32, i32> {
        let vif_idx = iface_id as usize;
        if vif_idx >= MAX_VIF {
            return Err(-22);
        }
        Ok(self.state.last_tx_power_dbm[vif_idx].unwrap_or(0) as i32 * 100)
    }

    fn get_channel(&mut self, iface_id: InterfaceId) -> Result<u8, i32> {
        let vif_idx = iface_id as usize;
        if vif_idx >= MAX_VIF {
            return Err(-22);
        }
        self.state.current_channel[vif_idx].ok_or(-61)
    }

    fn set_power_mgmt(&mut self, _iface_id: InterfaceId, _enabled: bool) -> Result<(), i32> {
        // 与 LicheeRV 一致：可选 MM_SET_PS_MODE_REQ；当前占位返回 Ok
        Ok(())
    }

    fn set_wiphy_params(&mut self, _rts: Option<i32>, _frag: Option<i32>) -> Result<(), i32> {
        Err(-38)
    }
}
