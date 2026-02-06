//! net_device 抽象：与 LicheeRV struct net_device 语义对齐
//!
//! Linux 中 net_device 提供 name、dev_addr、netdev_ops（ndo_open/ndo_stop/ndo_start_xmit）、stats。
//! StarryOS 无内核，此处提供最小结构体与 trait，供 FDRV/上层对接数据面与连接状态。

use core::result::Result;

/// MAC 地址长度
pub const ETH_ALEN: usize = 6;

/// 网卡统计（对应 struct net_device_stats）
#[derive(Debug, Clone, Default)]
pub struct NetDeviceStats {
    pub rx_packets: u32,
    pub tx_packets: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u32,
    pub tx_errors: u32,
    pub rx_dropped: u32,
    pub tx_dropped: u32,
}

/// net_device 抽象（对应 Linux struct net_device 核心字段）
/// 与 LicheeRV rwnx_vif->ndev、netif_carrier_on/off、netif_tx_start_all_queues 等逻辑对齐
#[derive(Debug, Clone)]
pub struct NetDevice {
    /// 接口名（如 "wlan0"）
    pub name: [u8; 16],
    /// MAC 地址（dev_addr）
    pub mac_addr: [u8; ETH_ALEN],
    /// 是否 up（netif_running）
    pub up: bool,
    /// 是否 carrier on（netif_carrier_ok）
    pub carrier_ok: bool,
    /// 统计
    pub stats: NetDeviceStats,
}

impl Default for NetDevice {
    fn default() -> Self {
        Self {
            name: [0; 16],
            mac_addr: [0; ETH_ALEN],
            up: false,
            carrier_ok: false,
            stats: NetDeviceStats::default(),
        }
    }
}

impl NetDevice {
    pub fn new(name: &str) -> Self {
        let mut n = Self::default();
        let bytes = name.as_bytes();
        let len = bytes.len().min(15);
        n.name[..len].copy_from_slice(&bytes[..len]);
        n
    }

    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    pub fn set_mac_addr(&mut self, mac: &[u8; ETH_ALEN]) {
        self.mac_addr.copy_from_slice(mac);
    }

    /// 对应 netif_carrier_on
    pub fn carrier_on(&mut self) {
        self.carrier_ok = true;
    }

    /// 对应 netif_carrier_off
    pub fn carrier_off(&mut self) {
        self.carrier_ok = false;
    }

    /// 对应 netif_tx_start_all_queues
    #[inline]
    pub fn tx_start_all_queues(&mut self) {
        // 无队列时仅表示“可发”
    }

    /// 对应 netif_tx_stop_all_queues
    #[inline]
    pub fn tx_stop_all_queues(&mut self) {}
}

/// 数据包发送入口（对应 ndo_start_xmit / dev_hard_start_xmit）
/// 返回 Ok(()) 表示已接管 skb；Err 为负错误码
pub trait NetDeviceXmit {
    fn start_xmit(&self, buf: &[u8]) -> Result<(), i32>;
}
