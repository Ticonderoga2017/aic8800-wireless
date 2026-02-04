//! # ieee80211 — IEEE 802.11 / cfg80211 / mac80211 抽象
//!
//! 完整复刻 aic8800 依赖的 Linux 内核 **cfg80211** 与 **mac80211** 接口，
//! 便于 FDRV 与 LicheeRV rwnx_main/rwnx_msg_* 逻辑对齐。
//!
//! ## 模块与 Linux 对应
//!
//! | 模块      | Linux 位置                    | 说明 |
//! |-----------|-------------------------------|------|
//! | ieee80211 | include/linux/ieee80211.h     | 信道、频段、速率、帧类型、管理帧结构 |
//! | cfg80211  | net/cfg80211.h                | wiphy、cfg80211_ops、scan/connect/key/station/beacon 参数与回调 |
//! | mac80211  | net/mac80211.h                | ieee80211_hw、ieee80211_conf、supported_band、txq_params |

#![no_std]

pub mod cfg80211;
pub mod ieee80211;
pub mod mac80211;

pub use cfg80211::{
    BeaconData, BssInfo, BssParams, Cfg80211Ops, ChanDef, ConnectParams, ExternalAuthParams, Ifindex,
    KeyParams, KeyStatus, MgmtTxParams, Nl80211Iftype, ScanRequest, StationInfo, SurveyInfo,
    TxPowerSetting, WiphyParams,
};
pub use ieee80211::{Band, Channel, Rate, WlanEid};
pub use mac80211::{Conf, Hw, HtCap, SupportedBand, TxqParams, VhtCap};
