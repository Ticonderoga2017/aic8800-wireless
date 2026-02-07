# mmc — SDIO/MMC 总线抽象

对应 **Linux 内核 MMC 子系统**（`drivers/mmc/core`、`include/linux/mmc`），将 aic8800 所依赖的 MMC 功能模块抽象为 trait 与类型，便于 BSP 实现与 LicheeRV 逻辑对齐。

## 与 Linux 的对应关系

| 本 crate 模块 | Linux 头文件/实现 | 说明 |
|---------------|-------------------|------|
| `types` | `mmc/card.h`, `host.h`, `sdio_ids.h` | `SdioDeviceId`、`MmcIos`、`MmcBusWidth`、`sdio_class` |
| `host` | `mmc/host.h`、core 中 host 占用 | `MmcHost`：`claim_host`、`set_ios` |
| `card` | `mmc/card.h` | `MmcCard`（RCA） |
| `sdio_func` | `mmc/sdio_func.h`、`sdio_ops.c` | `SdioFunc`：`readb`/`writeb`、`readsb`/`writesb`、`set_block_size`、`enable_func` 等 |
| `driver` | `sdio_func.h` 中 `sdio_driver` | `SdioDriver`：`id_table`、`probe`、`remove` |

## aic8800 依赖的 API 映射

- `sdio_claim_host` / `sdio_release_host` → `MmcHost::claim_host`（Guard drop）或 `with_host_claimed`
- `sdio_readb` / `sdio_writeb` → `SdioFunc::readb` / `writeb`
- `sdio_readsb` / `sdio_writesb` → `SdioFunc::readsb` / `writesb`
- `sdio_set_block_size` → `SdioFunc::set_block_size`
- `sdio_enable_func` / `sdio_disable_func` → `SdioFunc::enable_func` / `disable_func`
- `sdio_claim_irq` / `sdio_release_irq` → `SdioFunc::claim_irq` / `release_irq`（软中断时可空实现）
- `host->ops->set_ios(clock, bus_width)` → `MmcHost::set_ios`
- `sdio_register_driver`、id_table、probe → `SdioDriver`

## 使用

- **wireless** 与 **bsp** 依赖本 crate；BSP 实现 `MmcHost` 与 `SdioFunc`，供 FDRV 或上层通过 `mmc` 接口访问 SDIO。
- 详细对照见 `wireless/docs/aic8800_MMC调用与wireless_逐项对照.md`。
