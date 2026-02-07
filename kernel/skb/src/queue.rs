//! SkbQueue — 对应 Linux `struct sk_buff_head` 的 FIFO 队列
//!
//! 用于 RX 帧队列（aicwf_frame_enq / aicwf_frame_dequeue）、TX 聚合等。

use alloc::collections::VecDeque;

use super::SkBuff;

/// skb 的 FIFO 队列，对应 LicheeRV `struct sk_buff_head` + `skb_queue_tail` / `__skb_dequeue`。
pub struct SkbQueue {
    queue: VecDeque<SkBuff>,
}

impl SkbQueue {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// 队尾入队。对应 `__skb_queue_tail`。
    pub fn push_tail(&mut self, skb: SkBuff) {
        self.queue.push_back(skb);
    }

    /// 队首出队。对应 `__skb_dequeue`。
    pub fn pop_head(&mut self) -> Option<SkBuff> {
        self.queue.pop_front()
    }

    /// 队首出队（从队尾取）。对应 `skb_dequeue_tail`。
    pub fn pop_tail(&mut self) -> Option<SkBuff> {
        self.queue.pop_back()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// 清空并丢弃所有 skb。
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

impl Default for SkbQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// 多优先级帧队列，对应 LicheeRV `struct frame_queue`（queuelist[8]）。
#[allow(dead_code)]
pub struct FrameQueue {
    /// 优先级数量（通常 8）
    num_prio: usize,
    /// 当前最高优先级（0 = 空）
    hi_prio: u16,
    queues: [SkbQueue; 8],
}

#[allow(dead_code)]
impl FrameQueue {
    pub const MAX_PRIO: usize = 8;

    pub fn new(num_prio: usize) -> Self {
        let n = num_prio.min(Self::MAX_PRIO);
        FrameQueue {
            num_prio: n,
            hi_prio: 0,
            queues: [
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
                SkbQueue::new(),
            ],
        }
    }

    /// 入队到指定优先级。对应 `aicwf_frame_enq(..., prio)`。
    pub fn enqueue(&mut self, skb: SkBuff, prio: usize) {
        let p = prio.min(self.num_prio.saturating_sub(1));
        if self.hi_prio == 0 || (p as u16) < self.hi_prio {
            self.hi_prio = p as u16;
        }
        self.queues[p].push_tail(skb);
    }

    /// 从最高优先级队列出队。对应 `aicwf_frame_dequeue`。
    pub fn dequeue(&mut self) -> Option<SkBuff> {
        while self.hi_prio < self.num_prio as u16 {
            let q = &mut self.queues[self.hi_prio as usize];
            if let Some(skb) = q.pop_head() {
                return Some(skb);
            }
            self.hi_prio += 1;
        }
        self.hi_prio = 0;
        None
    }

    pub fn is_empty(&self) -> bool {
        self.queues[..self.num_prio]
            .iter()
            .all(SkbQueue::is_empty)
    }
}
