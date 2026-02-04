//! WiFi 管理器
//! 对应 aicwf_manager.c, aicwf_manager.h
//!
//! 管理 WiFi 接口状态、Netlink 消息

use alloc::string::String;
use core::result::Result;

/// Netlink 消息类型 (对应 aicwf_manager.h AIC_NL_*_TYPE)
#[allow(dead_code)]
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetlinkMsgType {
    DaemonOn = 1,
    DaemonOff = 2,
    DaemonAlive = 3,
    DelSta = 4,
    NewSta = 5,
    IntfRpt = 6,
    StaRpt = 7,
    FrameRpt = 8,
    TimeTick = 9,
    PrivInfoCmd = 10,
    BSteerCmd = 11,
    BSteerBlockAdd = 12,
    BSteerBlockDel = 13,
    BSteerRoam = 14,
    GeneralCmd = 100,
    Customer = 101,
    ConfigUpdate = 102,
}

/// Netlink 协议 (对应 NL_AIC_PROTOCOL)
#[allow(dead_code)]
pub const NL_AIC_PROTOCOL: u32 = 29;

/// Netlink 最大消息大小
#[allow(dead_code)]
pub const NL_MAX_MSG_SIZE: usize = 768;

/// WiFi 接口状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum WifiState {
    Down = 0,
    Up = 1,
    Scanning = 2,
    Connecting = 3,
    Connected = 4,
    Disconnecting = 5,
}

/// WiFi 管理器
pub struct WifiManager {
    state: WifiState,
    interface_name: String,
}

impl WifiManager {
    pub fn new(interface: &str) -> Self {
        Self {
            state: WifiState::Down,
            interface_name: String::from(interface),
        }
    }

    pub fn state(&self) -> WifiState {
        self.state
    }

    pub fn set_state(&mut self, state: WifiState) {
        self.state = state;
    }

    pub fn interface_name(&self) -> &str {
        &self.interface_name
    }

    pub fn up(&mut self) -> Result<(), i32> {
        self.state = WifiState::Up;
        Ok(())
    }

    pub fn down(&mut self) -> Result<(), i32> {
        self.state = WifiState::Down;
        Ok(())
    }
}
