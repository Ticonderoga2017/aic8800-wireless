//! 命令管理
//! 对应 aic_bsp_driver.h 中的 rwnx_cmd_mgr
//! 实现：命令队列、请求-确认配对、超时、on_cfm 回调

/// IPC E2A 消息参数大小
pub const IPC_E2A_MSG_PARAM_SIZE: usize = 256;

/// E2A 确认消息最大长度（用于存储 cfm 的 param）
pub const RWNX_CMD_E2AMSG_LEN_MAX: usize = 256;

/// App 到 Emb 的消息
#[derive(Debug, Clone)]
#[repr(C)]
pub struct IpcE2AMsg {
    pub id: u16,
    pub dummy_dest_id: u16,
    pub dummy_src_id: u16,
    pub param_len: u16,
    pub pattern: u32,
    pub param: [u32; IPC_E2A_MSG_PARAM_SIZE],
}

impl Default for IpcE2AMsg {
    fn default() -> Self {
        Self {
            id: 0,
            dummy_dest_id: 0,
            dummy_src_id: 0,
            param_len: 0,
            pattern: 0,
            param: [0; IPC_E2A_MSG_PARAM_SIZE],
        }
    }
}

/// Emb 到 App 的消息头 (A2E)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct LmacMsgHeader {
    pub id: u16,
    pub dest_id: u16,
    pub src_id: u16,
    pub param_len: u16,
}

/// LMAC 消息最大长度（含 param）
pub const LMAC_MSG_MAX_LEN: usize = 1024;

/// A2E 消息：头 + 可变长 param，序列化后经 SDIO 发送
#[derive(Debug, Clone)]
pub struct LmacMsg {
    pub header: LmacMsgHeader,
    pub param: [u8; LMAC_MSG_MAX_LEN],
}

impl LmacMsg {
    pub fn new(id: u16, dest_id: u16, src_id: u16, param_len: u16) -> Self {
        Self {
            header: LmacMsgHeader {
                id,
                dest_id,
                src_id,
                param_len,
            },
            param: [0; LMAC_MSG_MAX_LEN],
        }
    }

    /// 序列化到 buf: [header 8 bytes][param param_len bytes]，返回总长度（DC/DW 等用）
    pub fn serialize(&self, buf: &mut [u8]) -> usize {
        let h = &self.header;
        buf[0..2].copy_from_slice(&h.id.to_le_bytes());
        buf[2..4].copy_from_slice(&h.dest_id.to_le_bytes());
        buf[4..6].copy_from_slice(&h.src_id.to_le_bytes());
        buf[6..8].copy_from_slice(&h.param_len.to_le_bytes());
        let plen = h.param_len as usize;
        if plen > 0 && buf.len() >= 8 + plen {
            buf[8..8 + plen].copy_from_slice(&self.param[..plen]);
        }
        8 + plen
    }

    /// Aic8801 序列化：与 LicheeRV rwnx_set_cmd_tx 一致，前 8 字节为 [len+4 LE2, 0x11, 0x00, dummy4]，再接 id/dest/src/param_len/param
    /// 总长度 = 8 + 8 + param_len = 16 + param_len
    pub fn serialize_8801(&self, buf: &mut [u8]) -> usize {
        let h = &self.header;
        let plen = h.param_len as usize;
        let payload_len = 8u16 + h.param_len; // sizeof(lmac_msg) + param_len
        let len_plus_4 = payload_len as u32 + 4;
        buf[0] = (len_plus_4 & 0xff) as u8;
        buf[1] = ((len_plus_4 >> 8) & 0x0f) as u8;
        buf[2] = 0x11;
        buf[3] = 0x00; // Aic8801 no crc
        buf[4..8].fill(0); // dummy word
        buf[8..10].copy_from_slice(&h.id.to_le_bytes());
        buf[10..12].copy_from_slice(&h.dest_id.to_le_bytes());
        buf[12..14].copy_from_slice(&h.src_id.to_le_bytes());
        buf[14..16].copy_from_slice(&h.param_len.to_le_bytes());
        if plen > 0 && buf.len() >= 16 + plen {
            buf[16..16 + plen].copy_from_slice(&self.param[..plen]);
        }
        16 + plen
    }
}

/// 任务ID (对应 aic_bsp_driver.h TASK_*)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TaskId {
    None = 0xFF,
    Mm = 0,
    Dbg,
    Scan,
    Tdls,
    Scanu,
    Me,
    Sm,
    Apm,
    Bam,
    Mesh,
    Rxu,
    LastEmb = 11,
    Api,
    Max,
}

/// 命令标志
pub mod cmd_flags {
    pub const NONBLOCK: u16 = 1 << 0;
    pub const REQ_CFM: u16 = 1 << 1;
    pub const WAIT_PUSH: u16 = 1 << 2;
    pub const WAIT_ACK: u16 = 1 << 3;
    pub const WAIT_CFM: u16 = 1 << 4;
    pub const DONE: u16 = 1 << 5;
}

/// 802.11 命令超时 (ms)
pub const RWNX_80211_CMD_TIMEOUT_MS: u32 = 6000;

/// 命令管理器最大挂起数
const CMD_MGR_MAX_PENDING: usize = 8;

/// 单条挂起命令
struct PendingCmd {
    reqid: u16,
    done: bool,
    cfm_len: usize,
    cfm_data: [u8; RWNX_CMD_E2AMSG_LEN_MAX],
}

impl Default for PendingCmd {
    fn default() -> Self {
        Self {
            reqid: 0,
            done: false,
            cfm_len: 0,
            cfm_data: [0; RWNX_CMD_E2AMSG_LEN_MAX],
        }
    }
}

/// 命令管理器：队列、请求-确认配对、超时
/// 用法：push() 得到 token -> 平台调用 tx_fn 发送 msg -> RX 路径解析到 E2A 后调用 on_cfm -> wait_done(token, poll) 返回
pub struct RwnxCmdMgr {
    slots: [Option<PendingCmd>; CMD_MGR_MAX_PENDING],
    #[allow(dead_code)]
    next_tkn: u16,
}

impl RwnxCmdMgr {
    pub const fn new() -> Self {
        Self {
            slots: [None, None, None, None, None, None, None, None],
            next_tkn: 0,
        }
    }

    /// 登记一条需要确认的命令，返回 token（slot 下标）
    pub fn push(&mut self, reqid: u16) -> Option<usize> {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                log::debug!(target: "wireless::bsp", "cmd_mgr push reqid=0x{:04x} token={}", reqid, i);
                *slot = Some(PendingCmd {
                    reqid,
                    done: false,
                    cfm_len: 0,
                    cfm_data: [0; RWNX_CMD_E2AMSG_LEN_MAX],
                });
                return Some(i);
            }
        }
        log::warn!(target: "wireless::bsp", "cmd_mgr push: no free slot, reqid=0x{:04x}", reqid);
        None
    }

    /// RX 路径收到 E2A 确认时调用：根据 msg_id 匹配 reqid，写入 cfm 并标记 done；
    /// 并通知等待方（对齐 LicheeRV 的 complete(&cmd->complete)），使 wait_done 可立即返回。
    pub fn on_cfm(&mut self, msg_id: u16, param: &[u8]) {
        for slot in self.slots.iter_mut() {
            if let Some(ref mut s) = *slot {
                if s.reqid == msg_id && !s.done {
                    let len = param.len().min(RWNX_CMD_E2AMSG_LEN_MAX);
                    s.cfm_data[..len].copy_from_slice(&param[..len]);
                    s.cfm_len = len;
                    s.done = true;
                    log::debug!(target: "wireless::bsp", "cmd_mgr on_cfm msg_id=0x{:04x} len={}", msg_id, len);
                    crate::sdio_irq::notify_wait_done();
                    crate::sdio_irq::notify_cmd_done(); // 中断式通知主线程：CFM 已到，可立即唤醒 wait_done_until
                    return;
                }
            }
        }
    }

    pub fn is_done(&self, token: usize) -> bool {
        if token >= CMD_MGR_MAX_PENDING {
            return false;
        }
        self.slots[token]
            .as_ref()
            .map(|s| s.done)
            .unwrap_or(false)
    }

    /// 取走 cfm 数据并释放 slot
    pub fn take_cfm(&mut self, token: usize, out: &mut [u8]) -> Option<usize> {
        if token >= CMD_MGR_MAX_PENDING {
            return None;
        }
        let slot = self.slots[token].take()?;
        if !slot.done {
            return None;
        }
        let len = slot.cfm_len.min(out.len());
        out[..len].copy_from_slice(&slot.cfm_data[..len]);
        Some(len)
    }

    /// 等待该 token 完成；poll_fn 由调用方提供（例如执行一次 bus_poll_rx 并 on_cfm）
    /// poll_fn 接收 &mut self，便于在 RX 路径中调用 on_cfm
    /// timeout_ms: 超时毫秒数
    ///
    /// ## 超时原因（与 LicheeRV IPC 对比）
    /// - LicheeRV：设备把 CFM 放入 rd_fifo 后会拉 SDIO 中断，主机在中断里读 BLOCK_CNT 得长度再 sdio_readsb；`is_done` 由 RX 线程收到 CFM 后调 `on_cfm` 置位。
    /// - 本实现：无中断，轮询时 `poll_fn` = poll_rx_one → recv_pkt；recv_pkt 读 F1 BLOCK_CNT，>0 才从 rd_fifo 读，=0 直接返回“无数据”。
    /// - 若**设备从未把 DBG_MEM_READ_CFM 写入 rd_fifo**（BLOCK_CNT 一直为 0），则 recv_pkt 恒返回 0，poll_rx_one 不会解析到 CFM，不会调 `on_cfm`，`is_done(token)` 恒为 false → 到 timeout_ms 后返回 Err(-62)。
    /// - 因此超时表示：请求已发出（cmd53_write 成功），但设备未回 CFM（可能 ROM 未处理该命令、消息格式不符、或硬件/电源/时钟问题）。
    /// 若提供 tick，每 500ms 调用一次 tick(waited_ms)，用于打 F1 BLOCK_CNT/FLOW_CTRL 等调试日志。
    pub fn wait_done(
        &mut self,
        token: usize,
        timeout_ms: u32,
        poll_fn: &mut dyn FnMut(&mut RwnxCmdMgr),
        mut tick: Option<&mut dyn FnMut(u32)>,
    ) -> Result<(), i32> {
        crate::sdio_irq::ensure_sdio_irq_registered();
        let mut waited_ms: u32 = 0;
        const POLL_INTERVAL_MS: u32 = 1;
        const TICK_EVERY_MS: u32 = 500;
        while waited_ms < timeout_ms {
            if self.is_done(token) {
                log::trace!(target: "wireless::bsp", "cmd_mgr wait_done token={} ok in {}ms", token, waited_ms);
                return Ok(());
            }
            poll_fn(self);
            if self.is_done(token) {
                log::trace!(target: "wireless::bsp", "cmd_mgr wait_done token={} ok in {}ms (after poll)", token, waited_ms);
                return Ok(());
            }
            if waited_ms > 0 && waited_ms % TICK_EVERY_MS == 0 {
                if let Some(ref mut t) = tick {
                    t(waited_ms);
                }
            }
            waited_ms += POLL_INTERVAL_MS;
            // 有 SDMMC IRQ 时用 wait_timeout 阻塞至中断或 1ms；无 IRQ 时用 sleep 让出 CPU 给 busrx 线程收包
            if crate::sdio_irq::use_sdio_irq() {
                crate::sdio_irq::wait_sdio_or_timeout(core::time::Duration::from_millis(POLL_INTERVAL_MS as u64));
            } else {
                axtask::sleep(core::time::Duration::from_millis(POLL_INTERVAL_MS as u64));
            }
        }
        log::warn!(target: "wireless::bsp", "cmd_mgr wait_done token={} timeout {}ms", token, timeout_ms);
        Err(-62) // -ETIMEDOUT
    }

    /// 等待直到 condition() 为 true 或超时。主线程在 CMD_DONE 队列上阻塞，由后台 busrx 在 on_cfm 时
    /// notify_cmd_done 唤醒。若提供 poll_fn 则每轮先执行（主线程每 1ms 主动收包，与 LicheeRV 一致）。
    /// 与 LicheeRV wait_for_completion_timeout(&cmd->complete) 对齐。
    pub fn wait_done_until(
        timeout_ms: u32,
        mut condition: impl FnMut() -> bool,
        mut tick: Option<&mut dyn FnMut(u32)>,
        mut poll_fn: Option<&mut dyn FnMut()>,
    ) -> Result<(), i32> {
        crate::sdio_irq::ensure_sdio_irq_registered();
        log::info!(target: "wireless::bsp", "cmd_mgr wait_done_until: block for CFM (timeout {}ms), busrx will notify on reply", timeout_ms);
        let mut waited_ms: u32 = 0;
        const POLL_INTERVAL_MS: u64 = 1;
        const LOG_EVERY_MS: u32 = 500;
        let dur = core::time::Duration::from_millis(POLL_INTERVAL_MS);
        while waited_ms < timeout_ms {
            if let Some(ref mut pf) = poll_fn {
                pf();
            }
            if condition() {
                log::info!(target: "wireless::bsp", "cmd_mgr wait_done_until ok in {}ms", waited_ms);
                return Ok(());
            }
            if waited_ms > 0 && waited_ms % LOG_EVERY_MS == 0 {
                log::warn!(target: "wireless::bsp", "cmd_mgr wait_done_until: still waiting for CFM, {}ms/{}ms", waited_ms, timeout_ms);
                if let Some(ref mut t) = tick {
                    t(waited_ms);
                }
            }
            let _timed_out = crate::sdio_irq::wait_cmd_done_timeout(dur);
            waited_ms += POLL_INTERVAL_MS as u32;
        }
        log::warn!(target: "wireless::bsp", "cmd_mgr wait_done_until timeout {}ms (no CFM received)", timeout_ms);
        Err(-62) // -ETIMEDOUT
    }
}
