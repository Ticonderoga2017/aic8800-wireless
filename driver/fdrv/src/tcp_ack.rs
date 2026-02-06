//! TCP ACK 滤波：与 LicheeRV aicwf_tcp_ack.c / aicwf_tcp_ack.h 对齐
//!
//! 用于在发送路径上合并/延迟纯 ACK 包以省电；接收路径上标记 PSH 以提前发送待合并 ACK。
//! 数据结构与常量与 aicwf_tcp_ack.h 一致；无 Linux 时可为 stub（filter 返回 0 表示不滤波）。

/// 与 aicwf_tcp_ack.h 对齐
pub const TCP_ACK_NUM: usize = 32;
pub const TCP_ACK_DROP_CNT: u32 = 10;
pub const ACK_OLD_TIME_MS: u32 = 4000;
pub const MAX_TCP_ACK: usize = 200;
pub const MIN_WIN_KB: u32 = 256;
pub const SIZE_KB: u32 = 1024;

/// U32_BEFORE(a, b) = ((s32)((u32)a - (u32)b) <= 0)
#[inline]
pub fn u32_before(a: u32, b: u32) -> bool {
    (a as i32).wrapping_sub(b as i32) <= 0
}

/// 对应 struct tcp_ack_msg
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct TcpAckMsg {
    pub source: u16,
    pub dest: u16,
    pub saddr: i32,
    pub daddr: i32,
    pub seq: u32,
    pub win: u16,
}

/// 对应 struct tcp_ack_info（无 timer/seqlock 时仅保留业务字段）
#[derive(Debug, Clone, Default)]
pub struct TcpAckInfo {
    pub ack_info_num: usize,
    pub busy: u8,
    pub drop_cnt: u32,
    pub psh_flag: u8,
    pub psh_seq: u32,
    pub win_scale: u16,
    pub last_time_ms: u64,
    pub timeout_ms: u64,
    pub msgbuf: Option<*mut ()>,
    pub in_send_msg: Option<*mut ()>,
    pub ack_msg: TcpAckMsg,
}

/// 对应 struct tcp_ack_manage
#[derive(Debug, Clone, Default)]
pub struct TcpAckManage {
    pub enable: bool,
    pub max_num: usize,
    pub free_index: i32,
    pub last_time_ms: u64,
    pub timeout_ms: u64,
    pub max_drop_cnt: u32,
    pub ack_info: [TcpAckInfo; TCP_ACK_NUM],
    pub ack_winsize_kb: u32,
}

impl TcpAckManage {
    pub fn new() -> Self {
        let mut m = Self::default();
        m.max_drop_cnt = TCP_ACK_DROP_CNT;
        m.timeout_ms = ACK_OLD_TIME_MS as u64;
        m.ack_winsize_kb = MIN_WIN_KB;
        for (i, a) in m.ack_info.iter_mut().enumerate() {
            a.ack_info_num = i;
            a.timeout_ms = ACK_OLD_TIME_MS as u64;
        }
        m
    }
}

/// 发送路径 TCP ACK 滤波（对应 filter_send_tcp_ack）
/// 返回 0 表示不滤波、直接发送；1 表示已滤波（由内部定时/合并后发送）
/// Stub：无完整 TCP 解析时直接返回 0
#[inline]
pub fn filter_send_tcp_ack(_buf: &[u8], _plen: usize) -> i32 {
    0
}

/// 接收路径标记 PSH（对应 filter_rx_tcp_ack）
/// 在收到 TCP 数据时调用，用于更新 ack_info 的 psh_flag/psh_seq，便于发送路径提前发 ACK
#[inline]
pub fn filter_rx_tcp_ack(_buf: &[u8], _plen: usize) {
    // stub
}
