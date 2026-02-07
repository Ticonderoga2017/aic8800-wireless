//! SkBuff — 对应 Linux `struct sk_buff` 的包缓冲
//!
//! 布局：`[ headroom | data (len) | tailroom ]`，与 LicheeRV `skb->data`、`skb_put`、`skb_pull`、`skb_push` 语义一致。

use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

/// 单包缓冲，与 LicheeRV `struct sk_buff` 语义对齐。
///
/// - `data`：当前有效载荷起始（head 之后）
/// - `len`：有效载荷长度
/// - headroom：data 前的预留字节；tailroom：data 末之后的剩余空间
/// - `put(n)`：在尾部追加 n 字节（tailroom 减少）；对应 `skb_put`
/// - `pull(n)`：从头部消费 n 字节（data 前移，len 减少）；对应 `skb_pull`
/// - `push(n)`：在 data 前预留 n 字节（headroom 减少，len 增加）；对应 `skb_push`
#[derive(Clone)]
pub struct SkBuff {
    /// 整块存储： [0..head] = headroom, [head..head+len] = data, [head+len..] = tailroom
    storage: Vec<u8>,
    /// data 区在 storage 中的起始下标
    head: usize,
    /// 当前有效 data 长度
    len: usize,
}

impl SkBuff {
    /// 分配指定总容量的缓冲，可选 headroom；初始 data 长度 0。
    /// 对应 `__dev_alloc_skb(size, GFP_*)` / `dev_alloc_skb(size)`。
    pub fn alloc(capacity: usize) -> Self {
        Self::alloc_with_headroom(capacity, 0)
    }

    /// 分配容量并在前端预留 headroom 字节。
    pub fn alloc_with_headroom(capacity: usize, headroom: usize) -> Self {
        let head = headroom.min(capacity);
        let mut storage = Vec::with_capacity(capacity);
        storage.resize(capacity, 0);
        SkBuff {
            storage,
            head,
            len: 0,
        }
    }

    /// 当前有效载荷（data 区）只读视图。
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.storage[self.head..self.head + self.len]
    }

    /// 当前有效载荷可写视图（用于 recv 写入后配合 `set_len`）。
    #[inline]
    pub fn data_mut(&mut self) -> &mut [u8] {
        let end = self.head + self.storage.len().saturating_sub(self.head);
        &mut self.storage[self.head..end]
    }

    /// 从 data 起始处起、长度为 `len` 的切片（用于解析 E2A 等）；若 `len` 超出当前 data 长度则返回整个 data。
    #[inline]
    pub fn data_len(&self, len: usize) -> &[u8] {
        let n = len.min(self.len);
        &self.storage[self.head..self.head + n]
    }

    /// 设置当前有效 data 长度（收包后调用，如 LicheeRV 的 `skbbuf->len = size`）。
    #[inline]
    pub fn set_len(&mut self, len: usize) {
        let max = self.storage.len().saturating_sub(self.head);
        self.len = len.min(max);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// headroom 字节数（data 前的空间）。
    #[inline]
    pub fn headroom(&self) -> usize {
        self.head
    }

    /// tailroom 字节数（data 后的空间）。
    #[inline]
    pub fn tailroom(&self) -> usize {
        self.storage.len().saturating_sub(self.head + self.len)
    }

    /// 在尾部追加 n 字节，返回可写切片；空间不足则返回 None。对应 `skb_put(skb, n)`。
    #[inline]
    pub fn put(&mut self, n: usize) -> Option<&mut [u8]> {
        if self.tailroom() < n {
            return None;
        }
        let start = self.head + self.len;
        self.len += n;
        Some(&mut self.storage[start..start + n])
    }

    /// 从 data 头部消费 n 字节（data 指针前移、len 减少）。对应 `skb_pull(skb, n)`。
    #[inline]
    pub fn pull(&mut self, n: usize) {
        let consume = n.min(self.len);
        self.head += consume;
        self.len -= consume;
    }

    /// 在 data 前预留 n 字节（head 减少、len 增加）。对应 `skb_push(skb, n)`。
    #[inline]
    pub fn push(&mut self, n: usize) -> bool {
        if self.head < n {
            return false;
        }
        self.head -= n;
        self.len += n;
        true
    }

    /// 预留 headroom（分配时或确保 data 前至少有 n 字节）。当前实现仅在 alloc 时指定；此处为 API 兼容。
    #[inline]
    pub fn reserve(&mut self, n: usize) {
        if n <= self.head {
            return;
        }
        let need = n - self.head;
        let new_cap = self.storage.len() + need;
        let mut new_storage = Vec::with_capacity(new_cap);
        new_storage.resize(need, 0);
        new_storage.extend_from_slice(&self.storage[..]);
        self.storage = new_storage;
        self.head += need;
    }

    /// 将 data 区从偏移 `off` 起、长度 `n` 复制到 `dst`；若范围越界则复制有效部分。
    #[inline]
    pub fn copy_bits(&self, dst: &mut [u8], off: usize, n: usize) -> usize {
        let data = self.data();
        let start = off.min(data.len());
        let count = (data.len() - start).min(n).min(dst.len());
        dst[..count].copy_from_slice(&data[start..start + count]);
        count
    }
}

impl Deref for SkBuff {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.data()
    }
}

impl DerefMut for SkBuff {
    fn deref_mut(&mut self) -> &mut [u8] {
        let end = self.head + self.storage.len().saturating_sub(self.head);
        &mut self.storage[self.head..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skb_put_pull() {
        let mut skb = SkBuff::alloc_with_headroom(64, 4);
        assert_eq!(skb.headroom(), 4);
        assert_eq!(skb.len(), 0);
        let p = skb.put(8).unwrap();
        p.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(skb.len(), 8);
        assert_eq!(skb.data(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        skb.pull(2);
        assert_eq!(skb.data(), &[3, 4, 5, 6, 7, 8]);
        assert_eq!(skb.len(), 6);
    }
}
