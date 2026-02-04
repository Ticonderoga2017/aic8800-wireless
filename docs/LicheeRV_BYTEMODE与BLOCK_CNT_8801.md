# LicheeRV 中 8801 的 BYTEMODE 与 BLOCK_CNT 收包流程

本文档摘录 LicheeRV-Nano-Build 里 AIC8801 收包时 **BLOCK_CNT(0x12)** 与 **BYTEMODE_LEN(0x02)** 的用法，便于对照“小块数据、字节传输”是否有特殊处理。

---

## 1. 8801 的 RX 入口（仅看 BLOCK_CNT）

**文件**: `aic8800_bsp/aicsdio.c`，函数 `aicwf_sdio_hal_irqhandler`（约 1428–1473 行）

```c
if (sdiodev->chipid == PRODUCT_ID_AIC8801 || ... DC || ... DW) {
    ret = aicwf_sdio_readb(sdiodev, sdiodev->sdio_reg.block_cnt_reg, &intstatus);  // 只读 0x12

    while(intstatus){   // 仅当 block_cnt 非 0 才进入
        sdiodev->rx_priv->data_len = intstatus * SDIOWIFI_FUNC_BLOCKSIZE;  // 先按块算：data_len = block_cnt*512
        if (intstatus > 0) {
            if(intstatus < 64) {
                // 块数 < 64：直接用 data_len = block_cnt*512 去读 rd_fifo
                pkt = aicwf_sdio_readframes(sdiodev, 0);
            } else {
                // 块数 >= 64：再读 BYTEMODE_LEN(0x02)，用 byte_len*4 覆盖 data_len，再读
                aicwf_sdio_intr_get_len_bytemode(sdiodev, &byte_len);  // byte_len must<= 128
                sdio_info("byte mode len=%d\r\n", byte_len);
                pkt = aicwf_sdio_readframes(sdiodev, 0);
            }
        }
        if (pkt) aicwf_sdio_enq_rxpkt(sdiodev, pkt);
        ret = aicwf_sdio_readb(sdiodev, sdiodev->sdio_reg.block_cnt_reg, &intstatus);  // 再读 block_cnt
    }
}
```

**结论（8801）**：

- **有数据判断**：只看 `block_cnt_reg(0x12)`，`intstatus==0` 时直接不进入 `while`，不会去读 BYTEMODE_LEN。
- **小块（&lt; 1 个块）**：LicheeRV 侧没有“仅用 BYTEMODE_LEN、block_cnt 为 0”的路径。小块时通常是 **block_cnt=1**，`data_len=512`，从 rd_fifo 读 512 字节，实际包长在 payload 里。
- **大块（≥64 块）**：才用 `aicwf_sdio_intr_get_len_bytemode` 读 **bytemode_len_reg(0x02)**，把 `data_len` 设为 `byte_len*4`，再读 rd_fifo。

即：**8801 在 LicheeRV 里没有“block_cnt 一直为 0、只靠 BYTEMODE_LEN 收小块”的路径**；收包触发始终是 BLOCK_CNT 非 0。

---

## 2. aicwf_sdio_intr_get_len_bytemode（读 0x02，单位 4 字节）

**文件**: `aic8800_bsp/aicsdio.c`（881–896 行），`aic8800_fdrv/aicwf_sdio.c`（1699–1714 行）有同构实现

```c
static int aicwf_sdio_intr_get_len_bytemode(struct aic_sdio_dev *sdiodev, u8 *byte_len)
{
    if (!byte_len) return -EBADE;
    if (sdiodev->bus_if->state == BUS_DOWN_ST) {
        *byte_len = 0;
    } else {
        ret = aicwf_sdio_readb(sdiodev, sdiodev->sdio_reg.bytemode_len_reg, byte_len);  // F1 0x02
        sdiodev->rx_priv->data_len = (*byte_len)*4;   // 长度单位：4 字节
    }
    return ret;
}
```

- **bytemode_len_reg** = 0x02（`aicwf_sdio.h`：`SDIOWIFI_BYTEMODE_LEN_REG`）。
- **含义**：`data_len = byte_len * 4`，只在 **block_cnt >= 64** 时被 8801 路径调用，用于覆盖之前的 `block_cnt*512`。

---

## 3. aicwf_sdio_readframes / aicwf_sdio_recv_pkt（按 data_len 读 rd_fifo）

- **readframes**：用 `sdiodev->rx_priv->data_len` 分配 skb，再调 `aicwf_sdio_recv_pkt(sdiodev, skb, size, msg)`。
- **recv_pkt**：`sdio_readsb(sdiodev->func, rd_fifo_addr, skbbuf->data, size)`，即按已设好的 `data_len` 从 F1 RD_FIFO(0x08) 读。

所以无论“块模式”还是“byte 模式”，**最终都是从 rd_fifo 读 `data_len` 字节**；区别只在于 `data_len` 来自 `block_cnt*512` 还是 `byte_len*4`。

---

## 4. 8800D80/D80X2 的“byte mode”特殊值（与 8801 不同）

**文件**: `aicsdio.c` 1474–1516 行

- D80/D80X2 用 **misc_int_status_reg**，用 **intstatus 的数值** 区分：
  - **intstatus == 120**：F1 byte mode，读 bytemode_len，再 readframes(0)。
  - **intmaskf2 == 127**：F2 byte mode，读 bytemode_len，再 readframes(1)。
  - 其他：block mode，`data_len = (intstatus & 0x7F) * 512` 或 `& 0x7` * 512。

**8801 没有 120/127 这类“byte mode 状态字”**，8801 只有“先 block_cnt，≥64 再 bytemode_len”这一种组合。

---

## 5. 小结（你要看的“小块/字节传输”）

| 项目 | 8801 在 LicheeRV |
|------|------------------|
| 有数据判断 | **仅 BLOCK_CNT(0x12)**，为 0 则不收包 |
| 小块（&lt;512 字节） | 仍由 **block_cnt=1** 表示，读 512 字节；无“仅 BYTEMODE_LEN、block_cnt=0”的路径 |
| BYTEMODE_LEN(0x02) | 仅在 **block_cnt ≥ 64** 时读，用于把 data_len 设为 byte_len*4 |
| 寄存器定义 | 0x02=BYTEMODE_LEN_REG，0x12=BLOCK_CNT_REG（aicwf_sdio.h） |

若你这边 **BLOCK_CNT 一直为 0**，在 LicheeRV 的 8801 逻辑下等价于“没有数据可收”；LicheeRV 没有实现“8801 仅用 BYTEMODE_LEN 表示小块、不设 BLOCK_CNT”的收包路径。若要支持那种行为，需要确认芯片是否真的会在小块时只更新 0x02 不更新 0x12，再在驱动里增加对应分支。
