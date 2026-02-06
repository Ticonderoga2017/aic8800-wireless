//! AIC8800 WiFi 全功能驱动 (FDRV)
//!
//! 对应 LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/aic8800_fdrv/
//!
//! 功能包括:
//! - 私有命令 (aic_priv_cmd) - 用户空间 ioctl 接口
//! - IPC 主机 (ipc_host) - 与固件通信
//! - WiFi 管理器 (aicwf_manager)
//! - SDIO Host (sdio_host) - 数据收发
//! - Vendor 命令 (aic_vendor) - nl80211 扩展

#![no_std]

extern crate alloc;

mod cfgfile;
mod e2a_dispatch;
mod ipc;
mod lmac_cmd;
mod manager;
mod net_device;
mod priv_cmd;
mod sdio_bus;
mod sdio_host;
mod tcp_ack;
mod vendor;
mod wiphy;
mod wiphy_impl;
mod txrxif;

pub use ipc::{ipc_handle_e2a_msg, ipc_send_cmd_sync, CMD_TX_BUF_SIZE, IpcHostCb};
pub use manager::{WifiManager, WifiState};
pub use priv_cmd::{AndroidWifiPrivCmd, PRIV_CMD_BUF_MAX};
pub use sdio_bus::{
    aicwf_sdio_exit_equiv, aicwf_sdio_probe_equiv, aicwf_sdio_register_equiv, BusOps, BusState,
    NX_TXQ_CNT, NX_TXDESC_CNT_MAX, SdioDev, SdioHostEnv, SdioReg, SDIO_ACTIVE_ST, SDIO_BUFFER_SIZE,
    SDIO_SLEEP_ST, SDIO_TAIL_LEN, SDIOWIFI_FUNC_BLOCKSIZE,
};
pub use sdio_host::SdioHost;
pub use vendor::{AIC_OUI, VendorSubcmd};
pub use wiphy::{
    IfaceType, InterfaceId, ScanResult, StationInfo, WiphyOps, WiphyOpsStub,
};
pub use wiphy_impl::WiphyOpsImpl;
pub use e2a_dispatch::{
    set_scan_result_cb, set_scan_done_cb, set_connect_result_cb, set_disconnect_cb,
    e2a_indication_handler,
};
pub use txrxif::{TxDataIf, RxDataCb, set_rx_data_cb, tx_data};
pub use net_device::{NetDevice, NetDeviceStats, NetDeviceXmit, ETH_ALEN};
pub use tcp_ack::{
    TcpAckManage, TcpAckInfo, TcpAckMsg, filter_send_tcp_ack, filter_rx_tcp_ack,
    TCP_ACK_NUM, TCP_ACK_DROP_CNT, MAX_TCP_ACK,
};
pub use e2a_dispatch::{set_ps_change_cb, set_rssi_status_cb, PsChangeCb, RssiStatusCb};
pub use lmac_cmd::{
    build_scanu_start_req, build_sm_connect_req, build_sm_disconnect_req,
    build_mm_add_if_req, build_mm_remove_if_req,
    build_mm_key_add_req, build_mm_key_del_req, build_apm_start_req, build_apm_stop_req,
    build_mm_get_sta_info_req,
    parse_scanu_start_cfm, parse_scanu_start_cfm_full, parse_scanu_result_ind, parse_scan_result_to_bss_info,
    parse_sm_connect_ind, parse_sm_disconnect_ind,
    parse_mm_add_if_cfm, parse_mm_key_add_cfm, parse_mm_get_sta_info_cfm, parse_apm_start_cfm,
    MacVifType, MmAddIfCfm, MmKeyAddCfm, MmGetStaInfoCfm, ApmStartCfm, ScanuStartCfm,
    ScanuResultInd, SmConnectInd, SmDisconnectInd, MmPsChangeInd, MmRssiStatusInd,
};
pub use txrxif::{
    SDIO_TYPE_DATA, SDIO_TYPE_CFG, SDIO_TYPE_CFG_CMD_RSP, SDIO_TYPE_CFG_DATA_CFM, SDIO_TYPE_CFG_PRINT,
    CMD_BUF_MAX, MAX_RXQLEN, RX_HWHRD_LEN, IPC_RXBUF_CNT, IPC_RXDESC_CNT,
    fdrv_rx_data_invoke,
};
pub use cfgfile::{
    parse_configfile, parse_karst_configfile, RwnxConfFile, RwnxKarstConf,
};
