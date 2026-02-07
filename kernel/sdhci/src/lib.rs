//! DMA 缓冲区分配，供 wireless BSP SDIO 大块传输使用（sdhci  crate，原 dma）。
//!
//! 与 LicheeRV 中 host 使用 DMA 进行 CMD53 数据阶段一致：分配物理连续、设备可访问的
//! 内存，供 SDMMC 控制器 SDMA 使用。假定内核为恒等映射（virt = phys）。

#![no_std]

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
