//! E2A 指示分发：将 BSP 收到的 E2A 消息按 msg_id 分发给 scan_done/connect_result/disconnect 等回调
//! 对照 aic8800 rwnx_msg_rx.c 中 SCANU_RESULT_IND / SM_CONNECT_IND / SM_DISCONNECT_IND 处理

use crate::lmac_cmd::{
    parse_mm_ps_change_ind, parse_mm_rssi_status_ind, parse_scanu_result_ind, parse_sm_connect_ind,
    parse_sm_disconnect_ind, MmPsChangeInd, MmRssiStatusInd, ScanuResultInd,
    SmConnectInd, SmDisconnectInd,
};
use bsp::{
    MM_PS_CHANGE_IND, MM_RSSI_STATUS_IND, SCANU_RESULT_IND, SCANU_START_CFM, SM_CONNECT_IND,
    SM_DISCONNECT_IND,
};
use core::sync::atomic::{AtomicPtr, Ordering};

/// 扫描单条结果回调：由上层注册，收到 SCANU_RESULT_IND 时调用。可用 lmac_cmd::parse_scan_result_to_bss_info(ind, param) 得到 BssInfo
pub type ScanResultCb = Option<unsafe fn(ind: &ScanuResultInd, param: &[u8])>;

/// 扫描完成回调：与 LicheeRV 一致，在收到 SCANU_START_CFM 时调用（rwnx_rx_scanu_start_cfm -> cfg80211_scan_done）
pub type ScanDoneCb = Option<unsafe fn()>;

/// 连接结果回调：收到 SM_CONNECT_IND 时调用
pub type ConnectResultCb = Option<unsafe fn(ind: &SmConnectInd)>;

/// 断开指示回调：收到 SM_DISCONNECT_IND 时调用
pub type DisconnectCb = Option<unsafe fn(ind: &SmDisconnectInd)>;

/// 省电状态变化：收到 MM_PS_CHANGE_IND 时调用（与 rwnx_rx_ps_change_ind 一致）
pub type PsChangeCb = Option<unsafe fn(ind: &MmPsChangeInd)>;
/// RSSI 状态：收到 MM_RSSI_STATUS_IND 时调用（与 rwnx_rx_rssi_status_ind 一致）
pub type RssiStatusCb = Option<unsafe fn(ind: &MmRssiStatusInd)>;

static SCAN_RESULT_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static SCAN_DONE_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static CONNECT_RESULT_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static DISCONNECT_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static PS_CHANGE_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
static RSSI_STATUS_CB: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// 注册扫描结果回调（FDRV 或上层在 init 时调用）
pub fn set_scan_result_cb(cb: ScanResultCb) {
    SCAN_RESULT_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 注册扫描完成回调
pub fn set_scan_done_cb(cb: ScanDoneCb) {
    SCAN_DONE_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 注册连接结果回调
pub fn set_connect_result_cb(cb: ConnectResultCb) {
    CONNECT_RESULT_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 注册断开指示回调
pub fn set_disconnect_cb(cb: DisconnectCb) {
    DISCONNECT_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 注册 MM_PS_CHANGE_IND 回调
pub fn set_ps_change_cb(cb: PsChangeCb) {
    PS_CHANGE_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 注册 MM_RSSI_STATUS_IND 回调
pub fn set_rssi_status_cb(cb: RssiStatusCb) {
    RSSI_STATUS_CB.store(
        cb.map(|f| f as *mut ()).unwrap_or(core::ptr::null_mut()),
        Ordering::Relaxed,
    );
}

/// 从 BSP 调用的 E2A 指示回调：根据 msg_id 分发到对应 handler
/// 由 fdrv 在初始化时通过 bsp::set_e2a_indication_cb(Some(e2a_indication_handler)) 注册
pub unsafe fn e2a_indication_handler(msg_id: u16, param: *const u8, param_len: usize) {
    if param.is_null() || param_len == 0 {
        return;
    }
    let param_slice = core::slice::from_raw_parts(param, param_len);
    match msg_id {
        SCANU_RESULT_IND => {
            if let Some(ind) = parse_scanu_result_ind(param_slice) {
                let cb = SCAN_RESULT_CB.load(Ordering::Relaxed);
                if !cb.is_null() {
                    let f: unsafe fn(&ScanuResultInd, &[u8]) = core::mem::transmute(cb);
                    f(&ind, param_slice);
                }
            }
        }
        SCANU_START_CFM => {
            // 与 LicheeRV rwnx_rx_scanu_start_cfm 一致：收到 CFM 表示扫描结束，通知上层 scan_done
            let cb = SCAN_DONE_CB.load(Ordering::Relaxed);
            if !cb.is_null() {
                let f: unsafe fn() = core::mem::transmute(cb);
                f();
            }
        }
        SM_CONNECT_IND => {
            if let Some(ind) = parse_sm_connect_ind(param_slice) {
                let cb = CONNECT_RESULT_CB.load(Ordering::Relaxed);
                if !cb.is_null() {
                    let f: unsafe fn(&SmConnectInd) = core::mem::transmute(cb);
                    f(&ind);
                }
            }
        }
        SM_DISCONNECT_IND => {
            if let Some(ind) = parse_sm_disconnect_ind(param_slice) {
                let cb = DISCONNECT_CB.load(Ordering::Relaxed);
                if !cb.is_null() {
                    let f: unsafe fn(&SmDisconnectInd) = core::mem::transmute(cb);
                    f(&ind);
                }
            }
        }
        MM_PS_CHANGE_IND => {
            if let Some(ind) = parse_mm_ps_change_ind(param_slice) {
                let cb = PS_CHANGE_CB.load(Ordering::Relaxed);
                if !cb.is_null() {
                    let f: unsafe fn(&MmPsChangeInd) = core::mem::transmute(cb);
                    f(&ind);
                }
            }
        }
        MM_RSSI_STATUS_IND => {
            if let Some(ind) = parse_mm_rssi_status_ind(param_slice) {
                let cb = RSSI_STATUS_CB.load(Ordering::Relaxed);
                if !cb.is_null() {
                    let f: unsafe fn(&MmRssiStatusInd) = core::mem::transmute(cb);
                    f(&ind);
                }
            }
        }
        _ => {}
    }
}
