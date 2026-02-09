//! LicheeRV sdio_io 层原样移植（drivers/mmc/core/sdio_io.c）
//!
//! 调用链：sdio_readsb/sdio_writesb → sdio_io_rw_ext_helper → mmc_io_rw_extended（由 host 实现）。
//! 本模块提供常量、拆分逻辑及 sdio_readsb/sdio_writesb 入口，backend 实现 MmcIoRwExtended 执行单次 CMD53。

use core::cmp::min;

/// 单次 CMD53 字节模式最大字节数（与 sdio_max_byte_size 对齐，通常 512）
pub const SDIO_MAX_BYTE_SIZE: usize = 512;

/// 块模式时块大小（func->cur_blksize，通常 512）
pub const SDIO_CUR_BLKSIZE: usize = 512;

/// 块模式单次 CMD53 最大块数：min(host->max_blk_count, 511)，此处取 511 与 LicheeRV 一致
pub const SDIO_MAX_BLOCKS_PER_CMD: u32 = 511;

/// 块模式条件：func->card->cccr.multi_block && (size > sdio_max_byte_size(func))
/// wireless 通过读 CCCR 0x06 SMB 位得到 multi_block；本常量表示“大于此 size 且 multi_block 时走块循环”
pub const SDIO_BLOCK_MODE_SIZE_THRESHOLD: usize = SDIO_MAX_BYTE_SIZE;

/// 地址合法性：与 sdio_io.c 一致，func->num > 7 由上层保证；addr 由 mmc_io_rw_extended 检查 0x1FFFF
pub const SDIO_ADDR_MASK: u32 = 0x1_FFFF;

// =============================================================================
// LicheeRV 层次：sdio_readsb/sdio_writesb → sdio_io_rw_ext_helper → mmc_io_rw_extended
// =============================================================================

/// 单次 CMD53（mmc_io_rw_extended）执行者，由 BSP host 实现。
///
/// - 字节模式：`blocks == 0`，传输 `blksz` 字节。
/// - 块模式：`blocks >= 1`，传输 `blocks * blksz` 字节。
pub trait MmcIoRwExtended {
    fn io_rw_extended_read(
        &self,
        fn_num: u32,
        addr: u32,
        incr_addr: bool,
        buf: &mut [u8],
        blocks: u32,
        blksz: u32,
    ) -> Result<(), i32>;
    fn io_rw_extended_write(
        &self,
        fn_num: u32,
        addr: u32,
        incr_addr: bool,
        buf: &[u8],
        blocks: u32,
        blksz: u32,
    ) -> Result<(), i32>;
}

/// 与 sdio_io.c sdio_io_rw_ext_helper 一致：先块模式循环，再字节模式扫尾。
/// 本 SoC：size>=cur 时走块模式，512 字节=1 block DMA，避免 init 路径 PIO 后再 DMA 导致首帧块写超时（与 minimal 纯 DMA 一致）。
pub fn sdio_io_rw_ext_helper<H: MmcIoRwExtended>(
    host: &H,
    write: bool,
    addr: u32,
    incr_addr: bool,
    buf: &mut [u8],
    fn_num: u32,
    multi_block: bool,
    cur_blksize: u16,
) -> Result<(), i32> {
    let size = buf.len();
    if size == 0 {
        return Ok(());
    }
    let cur = cur_blksize as usize;
    let mut offset = 0;
    if multi_block && size >= cur {
        let mut remainder = size;
        while remainder >= cur {
            let blocks = min(remainder / cur, SDIO_MAX_BLOCKS_PER_CMD as usize) as u32;
            let chunk = (blocks as usize) * cur;
            if write {
                host.io_rw_extended_write(fn_num, addr, incr_addr, &buf[offset..offset + chunk], blocks, cur_blksize as u32)?;
            } else {
                host.io_rw_extended_read(fn_num, addr, incr_addr, &mut buf[offset..offset + chunk], blocks, cur_blksize as u32)?;
            }
            offset += chunk;
            remainder -= chunk;
        }
    }
    while offset < size {
        let n = min(size - offset, SDIO_MAX_BYTE_SIZE);
        if write {
            host.io_rw_extended_write(fn_num, addr, incr_addr, &buf[offset..offset + n], 0, n as u32)?;
        } else {
            host.io_rw_extended_read(fn_num, addr, incr_addr, &mut buf[offset..offset + n], 0, n as u32)?;
        }
        offset += n;
    }
    Ok(())
}

/// 与 Linux sdio_readsb 一致：FIFO 读，incr_addr=0。
pub fn sdio_readsb<H: MmcIoRwExtended>(
    host: &H,
    func_num: u32,
    addr: u32,
    dst: &mut [u8],
    multi_block: bool,
    cur_blksize: u16,
) -> Result<(), i32> {
    sdio_io_rw_ext_helper(host, false, addr, false, dst, func_num, multi_block, cur_blksize)
}

/// 与 Linux sdio_writesb 一致：FIFO 写，incr_addr=0。
/// 本 SoC：size>=cur 时走块模式（512 字节=1 block DMA），避免 PIO 后再 DMA 超时。
pub fn sdio_writesb<H: MmcIoRwExtended>(
    host: &H,
    func_num: u32,
    addr: u32,
    src: &[u8],
    multi_block: bool,
    cur_blksize: u16,
) -> Result<(), i32> {
    let size = src.len();
    if size == 0 {
        return Ok(());
    }
    let cur = cur_blksize as usize;
    let mut offset = 0;
    if multi_block && size >= cur {
        let mut remainder = size;
        while remainder >= cur {
            let blocks = min(remainder / cur, SDIO_MAX_BLOCKS_PER_CMD as usize) as u32;
            let chunk = (blocks as usize) * cur;
            host.io_rw_extended_write(func_num, addr, false, &src[offset..offset + chunk], blocks, cur_blksize as u32)?;
            offset += chunk;
            remainder -= chunk;
        }
    }
    while offset < size {
        let n = min(size - offset, SDIO_MAX_BYTE_SIZE);
        host.io_rw_extended_write(func_num, addr, false, &src[offset..offset + n], 0, n as u32)?;
        offset += n;
    }
    Ok(())
}
