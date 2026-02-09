//! LicheeRV writesb/readsb 底层：DMA 缓冲、SDIO arg、cache 一致性、寄存器/流程常量。
//!
//! 与 LicheeRV-Nano-Build/linux_5.10 对齐：
//! - sdio_ops.c mmc_io_rw_extended → arg/blksz/blocks
//! - sdio_io.c sdio_io_rw_ext_helper → 块模式 511 块上限
//! - sdhci.c/h 寄存器偏移、BLK_SIZE/BLK_COUNT、下发顺序

#![no_std]

pub mod cache;
pub mod flow;
pub mod sdhci;
pub mod sdio_io;
pub mod sdio_ops;

use core::ptr::NonNull;
use spin::Mutex;

/// 单块 DMA 池大小（256KB），满足单次 CMD53 最多 511×512 字节
const DMA_POOL_SIZE: usize = 256 * 1024;
/// TRM SDMA_BUF_BDARY=0 表示 4K 边界，SDMA 起始地址须 4K 对齐（LicheeRV SDHCI 同理）
static DMA_POOL: Mutex<Option<()>> = Mutex::new(None);
// 整块 256KB、4K 对齐的缓冲区（Align4K 为 ZST，size_of=0 会除零，故用 [u8; SIZE] 包一层并 align(4096)）
#[repr(align(4096))]
struct DmaPool([u8; DMA_POOL_SIZE]);

static mut DMA_BUFFER: DmaPool = DmaPool([0u8; DMA_POOL_SIZE]);

/// 分配一块 DMA 缓冲区，供 SDIO 单次 CMD53 数据阶段使用。
///
/// 返回 `(虚拟地址, 物理地址)`；恒等映射下两者相等。调用方负责在传输完成后调用 `release_dma_buffer`。
/// 同一时刻仅支持一块未释放的分配。
pub fn alloc_dma_buffer(size: usize) -> Option<(NonNull<u8>, usize)> {
    if size == 0 || size > DMA_POOL_SIZE || size % 4 != 0 {
        return None;
    }
    let mut guard = DMA_POOL.lock();
    if guard.is_some() {
        return None;
    }
    *guard = Some(());
    let ptr = unsafe { core::ptr::addr_of_mut!(DMA_BUFFER.0).cast::<u8>() };
    let virt = NonNull::new(ptr)?;
    let phys = ptr as usize;
    Some((virt, phys))
}

/// 释放当前占用的 DMA 缓冲区（与 `alloc_dma_buffer` 配对）。
pub fn release_dma_buffer() {
    let mut guard = DMA_POOL.lock();
    *guard = None;
}
