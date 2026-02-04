//! 私有命令接口
//! 对应 aic_priv_cmd.c, aic_priv_cmd.h
//!
//! Android WiFi 私有命令，用于用户空间与驱动通信

use alloc::vec;
use alloc::vec::Vec;
use core::default::Default;

/// 私有命令缓冲区最大长度
pub const PRIV_CMD_BUF_MAX: usize = 4096;

/// Android WiFi 私有命令
#[derive(Debug, Clone)]
pub struct AndroidWifiPrivCmd {
    pub buf: Vec<u8>,
    pub used_len: usize,
    pub total_len: usize,
}

impl Default for AndroidWifiPrivCmd {
    fn default() -> Self {
        Self {
            buf: vec![0; PRIV_CMD_BUF_MAX],
            used_len: 0,
            total_len: PRIV_CMD_BUF_MAX,
        }
    }
}

impl AndroidWifiPrivCmd {
    pub fn new(total_len: usize) -> Self {
        Self {
            buf: vec![0; total_len],
            used_len: 0,
            total_len,
        }
    }

    pub fn from_buf(buf: &[u8]) -> Self {
        let mut cmd = Self::new(buf.len());
        cmd.buf[..buf.len()].copy_from_slice(buf);
        cmd.used_len = buf.len();
        cmd
    }

    /// 解析命令字符串 "cmd arg1 arg2 ..."
    /// 支持: SCAN, CONNECT <ssid>, DISCONNECT, STATUS, MACADDR, RSSI
    pub fn parse_args(&self) -> Vec<&str> {
        let s = core::str::from_utf8(&self.buf[..self.used_len]).unwrap_or("");
        s.split_whitespace().collect::<Vec<_>>()
    }
}
