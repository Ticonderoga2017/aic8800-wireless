# skb — Socket buffer 抽象

对应 LicheeRV aic8800 依赖的 **`linux/skbuff.h`**，提供与 `struct sk_buff` / `struct sk_buff_head` 语义对齐的包缓冲与队列，便于 BSP/FDRV 收发包路径与 LicheeRV 对照。

## 模块

| 类型 | 说明 | LicheeRV 对应 |
|------|------|----------------|
| **SkBuff** | 单包缓冲：`data`/`len`/`headroom`/`tailroom`，`put`/`pull`/`push`/`reserve` | `struct sk_buff`、`skb->data`、`skb_put`、`skb_pull`、`skb_push`、`dev_alloc_skb`、`dev_kfree_skb` |
| **SkbQueue** | FIFO 队列 | `struct sk_buff_head`、`__skb_queue_tail`、`__skb_dequeue` |
| **FrameQueue** | 多优先级帧队列 | `struct frame_queue`（queuelist[8]）、`aicwf_frame_enq`、`aicwf_frame_dequeue` |

## 在 wireless 中的使用

- **bsp**：`flow.rs` 中 `poll_rx_one` 使用 `SkBuff::alloc(IPC_RX_BUF_SIZE)` 作为 RX 缓冲，`recv_pkt(skb.data_mut(), ...)`、`skb.set_len(n)` 后以 `skb.data()` 解析 CFM，与 LicheeRV `skb_inblock->data`、`skbbuf->len` 语义一致。

## 依赖

- `#![no_std]`，仅依赖 `alloc`（Vec、VecDeque）。
