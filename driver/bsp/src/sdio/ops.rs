//! AIC8800 SDIO 设备接口
//! 对照 LicheeRV aicsdio.c：aicwf_sdio_readb/writeb、send_pkt/recv_pkt、readb_func2/writeb_func2、send_msg

use core::cmp::min;

use super::backend::Aic8800SdioHost;
use super::types::{ProductId, reg, reg_v3};

/// SDIO Function 1/2 基址（与 Linux SDIO_FBR_BASE 一致，aicsdio.c 使用 func 1 / func_msg 2）
const FUNC1_BASE: u32 = 0x100;
const FUNC2_BASE: u32 = 0x200;
/// 8800DC/DW send_msg 固定地址：Function 2 偏移 7（aicsdio.c aicwf_sdio_send_msg sdio_writesb(func_msg, 7, ...)）
const FUNC2_MSG_ADDR_OFFSET: u32 = 7;
/// Aic8801 发送前流控：与 LicheeRV aicwf_sdio_flow_ctrl 一致，fc_reg != 0 即通过（return ret=fc_reg），无 >thresh 判断
const FLOW_CTRL_THRESH: u8 = 0; // LicheeRV: if (fc_reg != 0) return ret;
/// 与 LicheeRV FLOW_CTRL_RETRY_COUNT(50) 一致；延时与 aicsdio.c 659-691 一致：count<30 udelay(200), 30..40 mdelay(1), 40..50 mdelay(10)
const FLOW_CTRL_RETRY_COUNT: u32 = 50;
/// 与 LicheeRV aicsdio.h BUFFER_SIZE(1536) 一致，用于 aicwf_sdio_tx_msg 条件 len < (buffer_cnt * BUFFER_SIZE)
const BUFFER_SIZE: usize = 1536;

/// AIC8800 SDIO 设备接口，与 aicwf_sdio_readb/writeb、send_pkt/recv_pkt、func2、send_msg 一一对应。
/// FDRV 通过本 trait 访问 BSP 提供的 Aic8800Sdio 实现。
pub trait SdioOps {
    /// Function 1 单字节写。`regaddr` 为 Function 1 内寄存器偏移（非完整 SDIO 地址）。
    fn writeb(&self, regaddr: u32, val: u8) -> Result<(), i32>;
    /// Function 1 单字节读。`regaddr` 为 Function 1 内寄存器偏移。
    fn readb(&self, regaddr: u32) -> Result<u8, i32>;
    /// Function 2 单字节读。`regaddr` 为 Function 2 内寄存器偏移。
    fn readb_func2(&self, _regaddr: u32) -> Result<u8, i32> {
        Err(-19)
    }
    /// Function 2 单字节写。
    fn writeb_func2(&self, _regaddr: u32, _val: u8) -> Result<(), i32> {
        Err(-19)
    }
    /// 按完整 SDIO 地址读一字节（高字节为 func，低字节为 offset）。
    fn read_byte(&self, addr: u32) -> Result<u8, i32> {
        let func = ((addr >> 8) & 7) as u8;
        let offset = addr & 0xFF;
        match func {
            1 => self.readb(offset),
            2 => self.readb_func2(offset),
            _ => Err(-22),
        }
    }
    /// 读一字节且 **始终 fn=0**（与 LicheeRV sdio_cis.c 一致，用于 CIS 路径）。
    fn read_byte_f0(&self, addr: u32) -> Result<u8, i32> {
        self.read_byte(addr)
    }
    /// 按指定 function 与 17 位寄存器地址读一字节（用于 F1 CIS：Linux MMC 用 func 读该 function 的 CIS）。
    fn read_byte_at_func(&self, _func_num: u8, _reg_addr: u32) -> Result<u8, i32> {
        Err(-22)
    }
    /// 按完整 SDIO 地址写一字节。
    fn write_byte(&self, addr: u32, val: u8) -> Result<(), i32> {
        let func = ((addr >> 8) & 7) as u8;
        let offset = addr & 0xFF;
        match func {
            1 => self.writeb(offset, val),
            2 => self.writeb_func2(offset, val),
            _ => Err(-22),
        }
    }
    /// 从读 FIFO 接收一包数据。`size` 为字节数，`msg` 非 0 表示 Function 2 读 FIFO。
    fn recv_pkt(&self, buf: &mut [u8], size: u32, msg: u8) -> Result<usize, i32>;
    /// 向写 FIFO 发送一包数据，`count` 为字节数。
    fn send_pkt(&self, buf: &[u8], count: usize) -> Result<usize, i32>;
    /// 向 Function 2 消息地址发送消息（aicsdio.c send_msg）。
    fn send_msg(&self, _buf: &[u8], _count: usize) -> Result<usize, i32> {
        Err(-19)
    }
    /// 从 `addr` 起连续读 `buf.len()` 字节到 `buf`（完整 SDIO 地址）。
    fn read_block(&self, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.read_byte(addr.wrapping_add(i as u32))?;
        }
        Ok(buf.len())
    }
    /// 从 `buf` 连续写 `buf.len()` 字节到 `addr` 起（完整 SDIO 地址）。
    fn write_block(&self, addr: u32, buf: &[u8]) -> Result<usize, i32> {
        for (i, &b) in buf.iter().enumerate() {
            self.write_byte(addr.wrapping_add(i as u32), b)?;
        }
        Ok(buf.len())
    }
}

/// **仅用于读 FBR/CIS 的 SdioOps**：只持主机引用，不依赖 ProductId，在识别芯片前使用。
/// 读 CIS 只需 `read_byte`，本类型直接委托 `host.read_byte(addr)`，不涉及 FIFO 偏移或 chipid。
#[derive(Debug)]
pub struct CisReadOps<'a> {
    host: &'a Aic8800SdioHost,
}

impl<'a> CisReadOps<'a> {
    pub fn new(host: &'a Aic8800SdioHost) -> Self {
        Self { host }
    }
}

impl SdioOps for CisReadOps<'_> {
    fn readb(&self, regaddr: u32) -> Result<u8, i32> {
        self.host.read_byte(FUNC1_BASE + regaddr)
    }
    fn writeb(&self, regaddr: u32, val: u8) -> Result<(), i32> {
        self.host.write_byte(FUNC1_BASE + regaddr, val)
    }
    fn readb_func2(&self, regaddr: u32) -> Result<u8, i32> {
        self.host.read_byte(FUNC2_BASE + regaddr)
    }
    fn writeb_func2(&self, regaddr: u32, val: u8) -> Result<(), i32> {
        self.host.write_byte(FUNC2_BASE + regaddr, val)
    }
    fn read_byte(&self, addr: u32) -> Result<u8, i32> {
        self.host.read_byte(addr)
    }
    fn read_byte_f0(&self, addr: u32) -> Result<u8, i32> {
        self.host.read_byte_f0(addr)
    }
    fn write_byte(&self, addr: u32, val: u8) -> Result<(), i32> {
        self.host.write_byte(addr, val)
    }
    fn read_byte_at_func(&self, func_num: u8, reg_addr: u32) -> Result<u8, i32> {
        self.host.read_byte_at_func(func_num, reg_addr)
    }
    fn recv_pkt(&self, _buf: &mut [u8], _size: u32, _msg: u8) -> Result<usize, i32> {
        Err(-19)
    }
    fn send_pkt(&self, _buf: &[u8], _count: usize) -> Result<usize, i32> {
        Err(-19)
    }
}

/// AIC8800 SDIO 设备：按 chipid 选 V1/V2 或 V3 寄存器布局，对照 aicwf_sdio_reg_init、aicwf_sdio_*。
/// 主机为 Aic8800SdioHost（基于 SG2002 SD1 的 CMD52/CMD53）。
#[derive(Debug)]
pub struct Aic8800Sdio {
    host: Aic8800SdioHost,
    product_id: ProductId,
    wr_fifo_offset: u8,
    rd_fifo_offset: u8,
}

impl Aic8800Sdio {
    /// 按 chipid 构造，与 aicwf_sdio_reg_init 一致：D80/D80X2 用 V3，其余用 V1/V2。
    ///
    /// # 参数
    /// - `host`: SDIO 主机（如 `Aic8800SdioHost::new_sd1()`）。
    /// - `chipid`: 产品 ID，用于选择 V1/V2 或 V3 寄存器布局；Aic8801 的 IPC 走 F1 wr/rd_fifo，DC/DW 走 F2 msg。
    pub fn new(host: Aic8800SdioHost, chipid: ProductId) -> Self {
        let (wr_fifo_offset, rd_fifo_offset) = match chipid {
            ProductId::Aic8800D80 | ProductId::Aic8800D80X2 => {
                (reg_v3::WR_FIFO_ADDR, reg_v3::RD_FIFO_ADDR)
            }
            _ => (reg::WR_FIFO_ADDR, reg::RD_FIFO_ADDR),
        };
        Self {
            host,
            product_id: chipid,
            wr_fifo_offset,
            rd_fifo_offset,
        }
    }

    /// V1/V2 布局（8801/8800DC/8800DW），与 aicsdio.h SDIOWIFI_WR_FIFO_ADDR/RD_FIFO_ADDR 一致。
    ///
    /// # 参数
    /// - `host`: SDIO 主机实例。
    pub fn new_v1_v2(host: Aic8800SdioHost) -> Self {
        Self {
            host,
            product_id: ProductId::Aic8800Dc,
            wr_fifo_offset: reg::WR_FIFO_ADDR,
            rd_fifo_offset: reg::RD_FIFO_ADDR,
        }
    }

    /// V3 布局（8800D80/8800D80X2），与 aicsdio.h SDIOWIFI_*_V3 一致。
    ///
    /// # 参数
    /// - `host`: SDIO 主机实例。
    pub fn new_v3(host: Aic8800SdioHost) -> Self {
        Self {
            host,
            product_id: ProductId::Aic8800D80,
            wr_fifo_offset: reg_v3::WR_FIFO_ADDR,
            rd_fifo_offset: reg_v3::RD_FIFO_ADDR,
        }
    }

    /// 显式指定 FIFO 偏移（与 fdrv::SdioReg 一致时使用）。
    ///
    /// # 参数
    /// - `host`: SDIO 主机实例。
    /// - `wr_fifo_offset`: Function 1 写 FIFO 寄存器偏移。
    /// - `rd_fifo_offset`: Function 1 读 FIFO 寄存器偏移。
    pub fn new_with_reg(host: Aic8800SdioHost, wr_fifo_offset: u8, rd_fifo_offset: u8) -> Self {
        Self {
            host,
            product_id: ProductId::Aic8800Dc,
            wr_fifo_offset,
            rd_fifo_offset,
        }
    }

    pub fn host(&self) -> &Aic8800SdioHost {
        &self.host
    }

    /// 当前产品 ID（用于 mmc::SdioFunc::device_id 等）
    pub fn product_id(&self) -> ProductId {
        self.product_id
    }
}

impl SdioOps for Aic8800Sdio {
    fn writeb(&self, regaddr: u32, val: u8) -> Result<(), i32> {
        self.host.write_byte(FUNC1_BASE + regaddr, val)
    }

    fn readb(&self, regaddr: u32) -> Result<u8, i32> {
        self.host.read_byte(FUNC1_BASE + regaddr)
    }

    fn readb_func2(&self, regaddr: u32) -> Result<u8, i32> {
        self.host.read_byte(FUNC2_BASE + regaddr)
    }

    fn writeb_func2(&self, regaddr: u32, val: u8) -> Result<(), i32> {
        self.host.write_byte(FUNC2_BASE + regaddr, val)
    }

    /// 与 LicheeRV 一致：CCCR/CIS 用 fn=0+17 位(host.read_byte)；F1/F2 用 fn+reg(host.read_byte_at_func)。
    /// 0x100-0x1FF → F1 reg；0x200-0x2FF → F2 reg；其余(0x09、0x109、0x0400、0x1000 等) → fn=0 17 位。
    fn read_byte(&self, addr: u32) -> Result<u8, i32> {
        if (0x100..0x200).contains(&addr) {
            self.host.read_byte_at_func(1, addr & 0xFF)
        } else if (0x200..0x300).contains(&addr) {
            self.host.read_byte_at_func(2, addr & 0xFF)
        } else {
            self.host.read_byte(addr)
        }
    }

    fn write_byte(&self, addr: u32, val: u8) -> Result<(), i32> {
        if (0x100..0x200).contains(&addr) {
            self.host.write_byte_at_func(1, addr & 0xFF, val)
        } else if (0x200..0x300).contains(&addr) {
            self.host.write_byte_at_func(2, addr & 0xFF, val)
        } else {
            self.host.write_byte(addr, val)
        }
    }

    fn recv_pkt(&self, buf: &mut [u8], size: u32, msg: u8) -> Result<usize, i32> {
        let n = size as usize;
        if n > buf.len() {
            return Err(-22);
        }
        // Aic8801：IPC 走 F1 rd_fifo；LicheeRV 在中断里先读 F1 BLOCK_CNT(0x12)，有数据再读 rd_fifo
        let base = if self.product_id == ProductId::Aic8801 {
            FUNC1_BASE + u32::from(self.rd_fifo_offset)
        } else if msg == 0 {
            FUNC1_BASE + u32::from(self.rd_fifo_offset)
        } else {
            FUNC2_BASE + u32::from(self.rd_fifo_offset)
        };
        // 无 SDIO 中断时轮询：8801 与 LicheeRV aicsdio.c 1449-1472 一致，先读 F1 BLOCK_CNT(0x12)，非 0 再定长并读 rd_fifo
        if self.product_id == ProductId::Aic8801 {
            // LicheeRV 8801/DC/DW：先读 block_cnt_reg(0x12)，while(intstatus){ data_len=intstatus*512; 若 intstatus>=64 再读 bytemode_len(0x02) 用 byte_len*4 }
            // 必须用 read_byte_at_func(1, reg)：backend.read_byte(addr) 始终 fn=0，若用 read_byte(0x112) 会误读 F0 导致恒得 0
            const BLOCKSIZE: usize = 512;
            const BYTEMODE_THRESH: u8 = 64; // LicheeRV: intstatus < 64 用 block_cnt*512，>=64 用 bytemode_len*4
            let block_cnt = self.host.read_byte_at_func(1, reg::BLOCK_CNT as u32)?;
            if block_cnt == 0 {
                return Ok(0);
            }
            let data_len = if block_cnt >= BYTEMODE_THRESH {
                let byte_len = self.host.read_byte_at_func(1, reg::BYTEMODE_LEN as u32)?;
                (byte_len as usize) * 4
            } else {
                (block_cnt as usize) * BLOCKSIZE
            };
            // 必须读满 data_len 以排空 RD_FIFO，否则芯片可能不响应下一包（LicheeRV 用 data_len 分配 skb 并读满）
            let read_len = min(n, data_len);
            log::info!(target: "wireless::bsp::sdio", "recv_pkt: F1 block_cnt(0x12)={} data_len={} read_len={}", block_cnt, data_len, read_len);
            return self.host.read_block(base, &mut buf[..read_len]);
        } else {
            self.host.read_block(base, &mut buf[..n])
        }
    }

    fn send_pkt(&self, buf: &[u8], count: usize) -> Result<usize, i32> {
        let addr = FUNC1_BASE + u32::from(self.wr_fifo_offset);
        self.host.write_block(addr, &buf[..count])
    }

    /// IPC 消息：Aic8801 走 F1 wr_fifo（aicsdio.c 8801 用 aicwf_sdio_send_pkt），8800DC/DW 走 F2 reg 7（send_msg）。
    /// Aic8801 发送前须等 F1 FLOW_CTRL(0x0A) 表示有缓冲空间（与 LicheeRV aicwf_sdio_flow_ctrl + aicwf_sdio_tx_msg 完全一致）：
    /// 50 次重试、递增延时；未就绪则返回 -110 不写 WR_FIFO，避免 CMD53 超时。
    fn send_msg(&self, buf: &[u8], count: usize) -> Result<usize, i32> {
        let addr = if self.product_id == ProductId::Aic8801 {
            // 与 LicheeRV aicwf_sdio_tx_msg(aicsdio.c 979-1001) 完全一致：buffer_cnt = flow_ctrl(); while ((buffer_cnt<=0 || (buffer_cnt>0 && len>buffer_cnt*BUFFER_SIZE)) && retry<10) { retry++; buffer_cnt = flow_ctrl(); }; 再判断 buffer_cnt>0 && len<(buffer_cnt*BUFFER_SIZE) 才 send_pkt
            let mut buffer_cnt: i32 = 0;
            for retry in 0..10u8 {
                let mut last_fc: u8 = 0;
                for i in 0..FLOW_CTRL_RETRY_COUNT {
                    let fc = self.host.read_byte_at_func(1, reg::FLOW_CTRL as u32)?;
                    last_fc = fc & reg::FLOWCTRL_MASK;
                    if last_fc != 0 {
                        break;
                    }
                    if i < 30 {
                        crate::delay_spin_us(200);
                    } else if i < 40 {
                        axtask::sleep(core::time::Duration::from_millis(1));
                    } else {
                        axtask::sleep(core::time::Duration::from_millis(10));
                    }
                }
                buffer_cnt = last_fc as i32;
                if buffer_cnt > 0 && count < buffer_cnt as usize * BUFFER_SIZE {
                    break;
                }
                if retry == 9 {
                    return Err(-110);
                }
            }
            if buffer_cnt <= 0 || count >= buffer_cnt as usize * BUFFER_SIZE {
                return Err(-110);
            }
            FUNC1_BASE + u32::from(self.wr_fifo_offset)
        } else {
            FUNC2_BASE + FUNC2_MSG_ADDR_OFFSET
        };
        self.host.write_block(addr, &buf[..count])
    }

    fn read_block(&self, addr: u32, buf: &mut [u8]) -> Result<usize, i32> {
        self.host.read_block(addr, buf)
    }

    fn write_block(&self, addr: u32, buf: &[u8]) -> Result<usize, i32> {
        self.host.write_block(addr, buf)
    }
}
