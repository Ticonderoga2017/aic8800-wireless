//! LMAC 命令构建与 E2A 解析
//!
//! 对照 aic8800 lmac_msg.h / rwnx_msg_tx.c：SCANU_*、SM_*、MM_KEY_*、APM_*、MM_GET_STA_INFO_* 的 REQ 构建与 CFM 解析。

use bsp::{
    LmacMsg, DRV_TASK_ID,
    MM_ADD_IF_REQ, MM_ADD_IF_CFM, MM_REMOVE_IF_REQ, MM_REMOVE_IF_CFM,
    MM_STA_ADD_REQ, MM_STA_ADD_CFM, MM_STA_DEL_REQ, MM_STA_DEL_CFM,
    MM_KEY_ADD_REQ, MM_KEY_ADD_CFM, MM_KEY_DEL_REQ, MM_GET_STA_INFO_REQ, MM_GET_STA_INFO_CFM,
    MM_SET_POWER_REQ, MM_SET_POWER_CFM,
    APM_START_REQ, APM_START_CFM, APM_STOP_REQ,
};
use ieee80211::MacCipherSuite;

/// TASK_SCANU = 4, TASK_SM = 6, TASK_MM = 0, TASK_APM = 7
const TASK_SCANU: u16 = 4;
const TASK_SM: u16 = 6;
const TASK_MM: u16 = 0;
const TASK_APM: u16 = 7;

/// MAC 密钥最大长度，与 lmac_mac.h MAC_SEC_KEY_LEN 一致
const MAC_SEC_KEY_LEN: usize = 32;
/// mac_rateset 数组长度
const MAC_RATESET_LEN: usize = 12;

/// 广播 BSSID（全 0xFF）
pub const MAC_BCST: [u8; 6] = [0xff; 6];

/// 最大 SSID 长度
const MAC_SSID_LEN: usize = 32;

/// SCAN_CHANNEL_MAX = MAC_DOMAINCHANNEL_24G_MAX + MAC_DOMAINCHANNEL_5G_MAX = 42
const SCAN_CHANNEL_MAX: usize = 42;
/// mac_chan_def 与 lmac_mac.h 一致：freq(u16), band(u8), flags(u8), tx_power(s8) = 5 bytes
const MAC_CHAN_DEF_SIZE: usize = 5;

/// scanu_start_req 布局与 lmac_msg.h scanu_start_req 一致：chan[], ssid[], bssid, add_ies, add_ie_len, vif_idx, chan_cnt, ssid_cnt, no_cck, duration
fn build_scanu_start_req_param(vif_idx: u8, duration_us: u32, param_out: &mut [u8]) -> usize {
    let mut off = 0;
    // chan[SCAN_CHANNEL_MAX]；全 0 时固件扫全信道
    let chan_size = SCAN_CHANNEL_MAX * MAC_CHAN_DEF_SIZE;
    if off + chan_size > param_out.len() {
        return 0;
    }
    param_out[off..off + chan_size].fill(0);
    off += chan_size;
    // ssid[3]: mac_ssid = length(u8) + array[32] = 33 * 3
    let ssid_size = 33 * 3;
    if off + ssid_size > param_out.len() {
        return 0;
    }
    param_out[off..off + ssid_size].fill(0);
    off += ssid_size;
    // bssid: 6 bytes (mac_addr as 3*u16 in FW, we use 6*u8)
    if off + 6 > param_out.len() {
        return 0;
    }
    param_out[off..off + 6].copy_from_slice(&MAC_BCST);
    off += 6;
    // add_ies (u32), add_ie_len (u16)
    if off + 6 > param_out.len() {
        return 0;
    }
    param_out[off..off + 6].fill(0);
    off += 6;
    // vif_idx (u8), chan_cnt (u8), ssid_cnt (u8), no_cck (bool -> u8)
    if off + 4 > param_out.len() {
        return 0;
    }
    param_out[off] = vif_idx;
    param_out[off + 1] = 0; // chan_cnt = 0: 固件常解释为“所有支持信道”
    param_out[off + 2] = 0;
    param_out[off + 3] = 0;
    off += 4;
    // duration (u32)
    if off + 4 > param_out.len() {
        return 0;
    }
    param_out[off..off + 4].copy_from_slice(&duration_us.to_le_bytes());
    off += 4;
    off
}

/// 构建 SCANU_START_REQ 消息（与 rwnx_send_scanu_req 对齐）
pub fn build_scanu_start_req(vif_idx: u8, duration_us: u32) -> Option<LmacMsg> {
    use bsp::SCANU_START_REQ;
    let mut param = [0u8; 400];
    let plen = build_scanu_start_req_param(vif_idx, duration_us, &mut param);
    if plen == 0 {
        return None;
    }
    let mut msg = LmacMsg::new(SCANU_START_REQ, TASK_SCANU, DRV_TASK_ID, plen as u16);
    msg.param[..plen].copy_from_slice(&param[..plen]);
    Some(msg)
}

/// 构建 SM_CONNECT_REQ 消息（与 rwnx_send_sm_connect_req 对齐）
/// TODO: 完整 flags/ie_len/ie_buf/auth_type 等，与 lmac_msg.h sm_connect_req 逐字段对齐
pub fn build_sm_connect_req(
    vif_idx: u8,
    ssid: &[u8],
    bssid: Option<&[u8; 6]>,
    chan_freq: Option<u16>,
) -> Option<LmacMsg> {
    use bsp::SM_CONNECT_REQ;
    let mut param = [0u8; 320];
    let mut off = 0;
    // ssid: length (u8) + array[32]
    if off + 1 + MAC_SSID_LEN > param.len() {
        return None;
    }
    let ssid_len = ssid.len().min(MAC_SSID_LEN);
    param[off] = ssid_len as u8;
    param[off + 1..off + 1 + ssid_len].copy_from_slice(&ssid[..ssid_len]);
    off += 1 + MAC_SSID_LEN;
    // bssid: 6 bytes
    if off + 6 > param.len() {
        return None;
    }
    if let Some(b) = bssid {
        param[off..off + 6].copy_from_slice(b);
    } else {
        param[off..off + 6].copy_from_slice(&MAC_BCST);
    }
    off += 6;
    // chan: mac_chan_def = freq(u16), band(u8), flags(u8), tx_power(s8)
    if off + 4 > param.len() {
        return None;
    }
    if let Some(f) = chan_freq {
        param[off..off + 2].copy_from_slice(&f.to_le_bytes());
        param[off + 2] = 0; // band
        param[off + 3] = 0; // flags
        // tx_power 在下一字节，共 5 字节？lmac 里 mac_chan_def 是 4 字节对齐
    }
    off += 4;
    // flags (u32), ctrl_port_ethertype (u16), ie_len (u16), listen_interval (u16)
    if off + 10 > param.len() {
        return None;
    }
    param[off..off + 10].fill(0);
    off += 10;
    // dont_wait_bcmc (bool), auth_type (u8), uapsd_queues (u8), vif_idx (u8)
    if off + 4 > param.len() {
        return None;
    }
    param[off] = 0;     // dont_wait_bcmc
    param[off + 1] = 0; // auth_type = open
    param[off + 2] = 0; // uapsd_queues
    param[off + 3] = vif_idx;
    off += 4;
    // ie_buf[64] = 256 bytes (u32_l[64])
    if off + 256 > param.len() {
        return None;
    }
    off += 256;
    let param_len = off;
    let mut msg = LmacMsg::new(SM_CONNECT_REQ, TASK_SM, DRV_TASK_ID, param_len as u16);
    msg.param[..param_len].copy_from_slice(&param[..param_len]);
    Some(msg)
}

// ========== MM_ADD_IF / MM_REMOVE_IF（与 rwnx_send_add_if / rwnx_send_remove_if 对齐）==========

/// 虚拟接口类型，与 lmac_msg.h mac_vif_type / NL80211_IFTYPE 对应
#[repr(u8)]
pub enum MacVifType {
    Sta = 0,
    Ibss = 1,
    Ap = 2,
    MeshPoint = 3,
    Monitor = 4,
    Unknown = 5,
}

/// 构建 MM_ADD_IF_REQ：type(u8), addr(6), p2p(u8)，与 lmac_msg.h mm_add_if_req 一致
pub fn build_mm_add_if_req(vif_type: MacVifType, mac_addr: &[u8; 6], p2p: bool) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_ADD_IF_REQ, TASK_MM, DRV_TASK_ID, 8);
    msg.param[0] = vif_type as u8;
    msg.param[1..7].copy_from_slice(mac_addr);
    msg.param[7] = if p2p { 1 } else { 0 };
    msg
}

/// MM_ADD_IF_CFM：status(u8), inst_nbr(u8)
#[derive(Debug, Clone, Copy)]
pub struct MmAddIfCfm {
    pub status: u8,
    pub inst_nbr: u8,
}

pub fn parse_mm_add_if_cfm(param: &[u8]) -> Option<MmAddIfCfm> {
    if param.len() < 2 {
        return None;
    }
    Some(MmAddIfCfm {
        status: param[0],
        inst_nbr: param[1],
    })
}

/// 构建 MM_REMOVE_IF_REQ：inst_nbr(u8)
pub fn build_mm_remove_if_req(inst_nbr: u8) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_REMOVE_IF_REQ, TASK_MM, DRV_TASK_ID, 1);
    msg.param[0] = inst_nbr;
    msg
}

/// 构建 SM_DISCONNECT_REQ（与 rwnx_send_sm_disconnect_req 对齐）
pub fn build_sm_disconnect_req(vif_idx: u8, reason_code: u16) -> LmacMsg {
    use bsp::SM_DISCONNECT_REQ;
    let mut msg = LmacMsg::new(SM_DISCONNECT_REQ, TASK_SM, DRV_TASK_ID, 3);
    msg.param[0..2].copy_from_slice(&reason_code.to_le_bytes());
    msg.param[2] = vif_idx;
    msg
}

/// 解析 SCANU_START_CFM：与 lmac_msg.h scanu_start_cfm 一致（vif_idx, status, result_cnt）
#[derive(Debug, Clone, Copy)]
pub struct ScanuStartCfm {
    pub vif_idx: u8,
    pub status: u8,
    pub result_cnt: u8,
}

pub fn parse_scanu_start_cfm(param: &[u8]) -> Option<u8> {
    parse_scanu_start_cfm_full(param).map(|c| c.status)
}

pub fn parse_scanu_start_cfm_full(param: &[u8]) -> Option<ScanuStartCfm> {
    if param.len() < 3 {
        return None;
    }
    Some(ScanuStartCfm {
        vif_idx: param[0],
        status: param[1],
        result_cnt: param[2],
    })
}

/// 解析 SCANU_RESULT_IND 前部：与 lmac_msg.h scanu_result_ind 一致；payload 为 802.11 管理帧
#[derive(Debug, Clone)]
pub struct ScanuResultInd {
    pub length: u16,
    pub center_freq: u16,
    pub rssi: i8,
    pub payload_offset: usize,
}

pub fn parse_scanu_result_ind(param: &[u8]) -> Option<ScanuResultInd> {
    if param.len() < 10 {
        return None;
    }
    let length = u16::from_le_bytes([param[0], param[1]]);
    let _framectrl = u16::from_le_bytes([param[2], param[3]]);
    let center_freq = u16::from_le_bytes([param[4], param[5]]);
    let _band = param[6];
    let _sta_idx = param[7];
    let _inst_nbr = param[8];
    let rssi = param[9] as i8;
    Some(ScanuResultInd {
        length,
        center_freq,
        rssi,
        payload_offset: 10,
    })
}

/// 从 SCANU_RESULT_IND 的 param 解析出 BssInfo（与 rwnx_msg_rx rwnx_rx_scanu_result_ind 对齐）
/// payload 为 802.11 管理帧：bssid 在固定头 16 字节处，SSID 在 variable IEs 中（EID 0）
pub fn parse_scan_result_to_bss_info(ind: &ScanuResultInd, param: &[u8]) -> Option<ieee80211::BssInfo> {
    let payload = param.get(ind.payload_offset..)?;
    if payload.len() < 22 {
        return None;
    }
    let mut bssid = [0u8; 6];
    bssid.copy_from_slice(payload.get(16..22)?);
    let freq = ind.center_freq as u32;
    let rssi = ind.rssi as i32;
    let mut ssid = [0u8; 32];
    let mut ssid_len: u8 = 0;
    const WLAN_EID_SSID: u8 = 0;
    const BEACON_PROBERESP_FIXED: usize = 22 + 8 + 2 + 2;
    let ie_start = payload.get(BEACON_PROBERESP_FIXED..)?;
    let mut i = 0;
    while i + 2 <= ie_start.len() {
        let id = ie_start[i];
        let len = ie_start[i + 1] as usize;
        if i + 2 + len > ie_start.len() {
            break;
        }
        if id == WLAN_EID_SSID && len <= 32 {
            ssid_len = len as u8;
            ssid[..ssid_len as usize].copy_from_slice(&ie_start[i + 2..i + 2 + len]);
            break;
        }
        i += 2 + len;
    }
    Some(ieee80211::BssInfo {
        bssid,
        freq,
        rssi,
        ssid,
        ssid_len,
    })
}

/// 解析 SM_CONNECT_IND：status_code(u16), bssid(6), roamed(u8), vif_idx(u8), ap_idx(u8)，与 lmac_msg.h sm_connect_ind 对齐
#[derive(Debug, Clone)]
pub struct SmConnectInd {
    pub status_code: u16,
    pub bssid: [u8; 6],
    pub vif_idx: u8,
    /// STA 表项索引（用于 get_station 发 MM_GET_STA_INFO_REQ）
    pub ap_idx: u8,
}

/// SM_CONNECT_CFM 仅含 status（lmac_msg.h sm_connect_cfm）
pub fn parse_sm_connect_cfm(param: &[u8]) -> Option<u8> {
    if param.is_empty() {
        return None;
    }
    Some(param[0])
}

pub fn parse_sm_connect_ind(param: &[u8]) -> Option<SmConnectInd> {
    if param.len() < 2 + 6 + 1 + 1 + 1 {
        return None;
    }
    let status_code = u16::from_le_bytes([param[0], param[1]]);
    let mut bssid = [0u8; 6];
    bssid.copy_from_slice(&param[2..8]);
    let _roamed = param[8];
    let vif_idx = param[9];
    let ap_idx = param[10];
    Some(SmConnectInd {
        status_code,
        bssid,
        vif_idx,
        ap_idx,
    })
}

/// 解析 SM_DISCONNECT_IND：reason_code(u16), vif_idx(u8)
#[derive(Debug, Clone)]
pub struct SmDisconnectInd {
    pub reason_code: u16,
    pub vif_idx: u8,
}

pub fn parse_sm_disconnect_ind(param: &[u8]) -> Option<SmDisconnectInd> {
    if param.len() < 3 {
        return None;
    }
    let reason_code = u16::from_le_bytes([param[0], param[1]]);
    let vif_idx = param[2];
    Some(SmDisconnectInd {
        reason_code,
        vif_idx,
    })
}

// ========== MM_KEY_ADD / MM_KEY_DEL（与 rwnx_send_key_add / rwnx_send_key_del 对齐）==========

/// mm_key_add_req 布局：key_idx(u8), sta_idx(u8), key.length(u8), key.array[32], cipher_suite(u8), inst_nbr(u8), spp(u8), pairwise(u8)
fn build_mm_key_add_req_param(
    key_idx: u8,
    sta_idx: u8,
    key: &[u8],
    cipher_suite: u8,
    inst_nbr: u8,
    pairwise: bool,
    param_out: &mut [u8],
) -> usize {
    let key_len = key.len().min(MAC_SEC_KEY_LEN);
    let mut off = 0;
    if param_out.len() < 2 + 1 + MAC_SEC_KEY_LEN + 4 {
        return 0;
    }
    param_out[off] = key_idx;
    off += 1;
    param_out[off] = sta_idx;
    off += 1;
    param_out[off] = key_len as u8;
    off += 1;
    param_out[off..off + key_len].copy_from_slice(&key[..key_len]);
    off += MAC_SEC_KEY_LEN;
    param_out[off] = cipher_suite;
    off += 1;
    param_out[off] = inst_nbr;
    off += 1;
    param_out[off] = 0; // spp
    off += 1;
    param_out[off] = if pairwise { 1 } else { 0 };
    off += 1;
    off
}

/// 构建 MM_KEY_ADD_REQ（与 rwnx_send_key_add 对齐）。sta_idx=0xFF 表示 default/group key。
pub fn build_mm_key_add_req(
    vif_idx: u8,
    key_idx: u8,
    sta_idx: u8,
    key: &[u8],
    cipher_suite: MacCipherSuite,
    pairwise: bool,
) -> Option<LmacMsg> {
    let mut param = [0u8; 64];
    let plen = build_mm_key_add_req_param(
        key_idx,
        sta_idx,
        key,
        cipher_suite as u8,
        vif_idx,
        pairwise,
        &mut param,
    );
    if plen == 0 {
        return None;
    }
    let mut msg = LmacMsg::new(MM_KEY_ADD_REQ, TASK_MM, DRV_TASK_ID, plen as u16);
    msg.param[..plen].copy_from_slice(&param[..plen]);
    Some(msg)
}

/// MM_KEY_ADD_CFM：status(u8), hw_key_idx(u8), aligned[2]
#[derive(Debug, Clone, Copy)]
pub struct MmKeyAddCfm {
    pub status: u8,
    pub hw_key_idx: u8,
}

pub fn parse_mm_key_add_cfm(param: &[u8]) -> Option<MmKeyAddCfm> {
    if param.len() < 2 {
        return None;
    }
    Some(MmKeyAddCfm {
        status: param[0],
        hw_key_idx: param[1],
    })
}

/// 构建 MM_KEY_DEL_REQ
pub fn build_mm_key_del_req(hw_key_idx: u8) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_KEY_DEL_REQ, TASK_MM, DRV_TASK_ID, 1);
    msg.param[0] = hw_key_idx;
    msg
}

/// MM_SET_POWER_REQ：与 lmac_msg.h mm_set_power_req 一致（inst_nbr, power s8）
pub fn build_mm_set_power_req(inst_nbr: u8, power_dbm: i8) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_SET_POWER_REQ, TASK_MM, DRV_TASK_ID, 2);
    msg.param[0] = inst_nbr;
    msg.param[1] = power_dbm as u8;
    msg
}

/// MM_SET_POWER_CFM：与 lmac_msg.h mm_set_power_cfm 一致（radio_idx, power s8）
#[derive(Debug, Clone)]
pub struct MmSetPowerCfm {
    pub radio_idx: u8,
    pub power: i8,
}

pub fn parse_mm_set_power_cfm(param: &[u8]) -> Option<MmSetPowerCfm> {
    if param.len() < 2 {
        return None;
    }
    Some(MmSetPowerCfm {
        radio_idx: param[0],
        power: param[1] as i8,
    })
}

/// MM_PS_CHANGE_IND：与 lmac_msg.h mm_ps_change_ind 一致（sta_idx, ps_state：0=active, 1=sleeping）
#[derive(Debug, Clone)]
pub struct MmPsChangeInd {
    pub sta_idx: u8,
    pub ps_state: u8,
}

pub fn parse_mm_ps_change_ind(param: &[u8]) -> Option<MmPsChangeInd> {
    if param.len() < 2 {
        return None;
    }
    Some(MmPsChangeInd {
        sta_idx: param[0],
        ps_state: param[1],
    })
}

/// MM_RSSI_STATUS_IND：与 lmac_msg.h mm_rssi_status_ind 一致（vif_index, rssi_status, rssi）
#[derive(Debug, Clone)]
pub struct MmRssiStatusInd {
    pub vif_index: u8,
    pub rssi_status: bool,
    pub rssi: i8,
}

pub fn parse_mm_rssi_status_ind(param: &[u8]) -> Option<MmRssiStatusInd> {
    if param.len() < 3 {
        return None;
    }
    Some(MmRssiStatusInd {
        vif_index: param[0],
        rssi_status: param[1] != 0,
        rssi: param[2] as i8,
    })
}

/// MM_STA_ADD_REQ 最小参数：与 lmac_msg.h mm_sta_add_req 对齐（mac_addr + inst_nbr；其余字段可填 0）
pub fn build_mm_sta_add_req(inst_nbr: u8, mac: &[u8; 6]) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_STA_ADD_REQ, TASK_MM, DRV_TASK_ID, 7);
    msg.param[0..6].copy_from_slice(mac);
    msg.param[6] = inst_nbr;
    msg
}

pub fn parse_mm_sta_add_cfm(param: &[u8]) -> Option<u8> {
    if param.is_empty() {
        return None;
    }
    Some(param[0])
}

/// MM_STA_DEL_REQ：与 lmac_msg.h mm_sta_del_req 一致（sta_idx）
pub fn build_mm_sta_del_req(sta_idx: u8) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_STA_DEL_REQ, TASK_MM, DRV_TASK_ID, 1);
    msg.param[0] = sta_idx;
    msg
}

pub fn parse_mm_sta_del_cfm(param: &[u8]) -> Option<u8> {
    if param.is_empty() {
        return None;
    }
    Some(param[0])
}

// ========== MM_GET_STA_INFO（与 rwnx_send_get_sta_info_req 对齐）==========

/// 构建 MM_GET_STA_INFO_REQ（简单版，仅 sta_idx；兼容版带 pattern 可后续加）
pub fn build_mm_get_sta_info_req(sta_idx: u8) -> LmacMsg {
    let mut msg = LmacMsg::new(MM_GET_STA_INFO_REQ, TASK_MM, DRV_TASK_ID, 1);
    msg.param[0] = sta_idx;
    msg
}

/// mm_get_sta_info_cfm 布局：rate_info(u32), txfailed(u32), rssi(u8), reserved[3], chan_time(u32), chan_busy_time(u32), ack_fail_stat(u32), ack_succ_stat(u32), chan_tx_busy_time(u32)
#[derive(Debug, Clone, Copy, Default)]
pub struct MmGetStaInfoCfm {
    pub rate_info: u32,
    pub txfailed: u32,
    pub rssi: i8,
    pub chan_time: u32,
    pub chan_busy_time: u32,
    pub ack_fail_stat: u32,
    pub ack_succ_stat: u32,
    pub chan_tx_busy_time: u32,
}

pub fn parse_mm_get_sta_info_cfm(param: &[u8]) -> Option<MmGetStaInfoCfm> {
    const CFM_LEN: usize = 4 + 4 + 1 + 3 + 4 * 5; // rate_info, txfailed, rssi, reserved[3], 5*u32
    if param.len() < CFM_LEN {
        return None;
    }
    let mut off = 0;
    let rate_info = u32::from_le_bytes([param[0], param[1], param[2], param[3]]);
    off += 4;
    let txfailed = u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    off += 4;
    let rssi = param[off] as i8;
    off += 1 + 3; // reserved[3]
    let chan_time = u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    off += 4;
    let chan_busy_time =
        u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    off += 4;
    let ack_fail_stat =
        u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    off += 4;
    let ack_succ_stat =
        u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    off += 4;
    let chan_tx_busy_time =
        u32::from_le_bytes([param[off], param[off + 1], param[off + 2], param[off + 3]]);
    Some(MmGetStaInfoCfm {
        rate_info,
        txfailed,
        rssi,
        chan_time,
        chan_busy_time,
        ack_fail_stat,
        ack_succ_stat,
        chan_tx_busy_time,
    })
}

// ========== APM_START / APM_STOP（与 rwnx_send_apm_start_req / rwnx_send_apm_stop_req 对齐）==========

/// apm_start_req 最小布局：basic_rates(length+array[12]), chan(freq u16, band u8, flags u8, tx_power s8), center_freq1(u32), center_freq2(u32), ch_width(u8), bcn_addr(u32), bcn_len(u16), tim_oft(u16), bcn_int(u16), flags(u32), ctrl_port_ethertype(u16), tim_len(u8), vif_idx(u8)
fn build_apm_start_req_param(
    vif_idx: u8,
    chan_freq_mhz: u16,
    chan_band: u8,
    bcn_int: u16,
    basic_rates: &[u8],
    bcn_addr: u32,
    bcn_len: u16,
    tim_oft: u16,
    tim_len: u8,
    param_out: &mut [u8],
) -> usize {
    let mut off = 0;
    // mac_rateset
    if param_out.len() < off + 1 + MAC_RATESET_LEN {
        return 0;
    }
    let rate_len = basic_rates.len().min(MAC_RATESET_LEN);
    param_out[off] = rate_len as u8;
    off += 1;
    param_out[off..off + rate_len].copy_from_slice(&basic_rates[..rate_len]);
    off += MAC_RATESET_LEN;
    // mac_chan_def: freq(u16), band(u8), flags(u8), tx_power(s8)
    if param_out.len() < off + 4 {
        return 0;
    }
    param_out[off..off + 2].copy_from_slice(&chan_freq_mhz.to_le_bytes());
    off += 2;
    param_out[off] = chan_band;
    off += 1;
    param_out[off] = 0;
    off += 1;
    param_out[off] = 20i8 as u8; // tx_power 默认
    off += 1;
    // center_freq1, center_freq2, ch_width
    if param_out.len() < off + 4 + 4 + 1 {
        return 0;
    }
    let cf1 = chan_freq_mhz as u32;
    param_out[off..off + 4].copy_from_slice(&cf1.to_le_bytes());
    off += 4;
    param_out[off..off + 4].copy_from_slice(&cf1.to_le_bytes());
    off += 4;
    param_out[off] = 0; // ch_width 20MHz
    off += 1;
    // bcn_addr, bcn_len, tim_oft, bcn_int
    if param_out.len() < off + 4 + 2 + 2 + 2 {
        return 0;
    }
    param_out[off..off + 4].copy_from_slice(&bcn_addr.to_le_bytes());
    off += 4;
    param_out[off..off + 2].copy_from_slice(&bcn_len.to_le_bytes());
    off += 2;
    param_out[off..off + 2].copy_from_slice(&tim_oft.to_le_bytes());
    off += 2;
    param_out[off..off + 2].copy_from_slice(&bcn_int.to_le_bytes());
    off += 2;
    // flags, ctrl_port_ethertype, tim_len, vif_idx
    if param_out.len() < off + 4 + 2 + 1 + 1 {
        return 0;
    }
    param_out[off..off + 4].fill(0);
    off += 4;
    param_out[off..off + 2].copy_from_slice(&0x888Eu16.to_le_bytes()); // ETH_P_PAE
    off += 2;
    param_out[off] = tim_len;
    off += 1;
    param_out[off] = vif_idx;
    off += 1;
    off
}

/// 构建 APM_START_REQ（最小实现：vif_idx、信道、bcn_int、basic_rates；bcn_addr 可为 0，由固件或后续 APM_SET_BEACON_IE 填充）
pub fn build_apm_start_req(
    vif_idx: u8,
    channel: u8,
    beacon_interval: u16,
    basic_rates: &[u8],
) -> Option<LmacMsg> {
    let chan_freq = ieee80211_channel_to_freq(channel);
    let mut param = [0u8; 128];
    let plen = build_apm_start_req_param(
        vif_idx,
        chan_freq,
        0,   // band 2.4G
        beacon_interval,
        basic_rates,
        0,   // bcn_addr
        0,   // bcn_len
        0,   // tim_oft
        0,   // tim_len
        &mut param,
    );
    if plen == 0 {
        return None;
    }
    let mut msg = LmacMsg::new(APM_START_REQ, TASK_APM, DRV_TASK_ID, plen as u16);
    msg.param[..plen].copy_from_slice(&param[..plen]);
    Some(msg)
}

fn ieee80211_channel_to_freq(ch: u8) -> u16 {
    if ch >= 1 && ch <= 13 {
        return 2407 + (ch as u16) * 5;
    }
    if ch >= 36 && ch <= 165 {
        return 5000 + (ch as u16) * 5;
    }
    2412
}

/// APM_START_CFM：status(u8), vif_idx(u8), ch_idx(u8), bcmc_idx(u8)
#[derive(Debug, Clone, Copy)]
pub struct ApmStartCfm {
    pub status: u8,
    pub vif_idx: u8,
    pub ch_idx: u8,
    pub bcmc_idx: u8,
}

pub fn parse_apm_start_cfm(param: &[u8]) -> Option<ApmStartCfm> {
    if param.len() < 4 {
        return None;
    }
    Some(ApmStartCfm {
        status: param[0],
        vif_idx: param[1],
        ch_idx: param[2],
        bcmc_idx: param[3],
    })
}

/// 构建 APM_STOP_REQ
pub fn build_apm_stop_req(vif_idx: u8) -> LmacMsg {
    let mut msg = LmacMsg::new(APM_STOP_REQ, TASK_APM, DRV_TASK_ID, 1);
    msg.param[0] = vif_idx;
    msg
}
