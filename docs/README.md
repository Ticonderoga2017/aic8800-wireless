# StarryOS wireless 文档

本目录存放 LicheeRV-Nano-Build WiFi 模块内核功能分析与移植到 StarryOS/wireless 的说明文档。

## 文档列表

| 文档 | 内容 |
|------|------|
| [LicheeRV_WiFi_Kernel_移植分析.md](./LicheeRV_WiFi_Kernel_移植分析.md) | 内核功能总览、BSP/FDRV 详解、互动机制、实现机制、移植方案与附录 |
| [cfg80211_ops_对照表.md](./cfg80211_ops_对照表.md) | cfg80211_ops 与 StarryOS 无线控制平面 trait 的逐项对照与 MVP 建议 |

## 阅读顺序建议

1. 先读 **LicheeRV_WiFi_Kernel_移植分析.md** 了解整体架构与移植思路。
2. 设计或实现“无线控制平面”时参考 **cfg80211_ops_对照表.md** 做接口一一对应。

## 相关代码位置

- **LicheeRV-Nano-Build WiFi 源码**：`LicheeRV-Nano-Build/osdrv/extdrv/wireless/aic8800/`
- **StarryOS wireless**：`StarryOS/wireless/`（含 `driver/bsp`、`driver/fdrv`）

## 调试日志

wireless 使用标准 [log](https://docs.rs/log) 门面，在 BSP/FDRV 关键路径打点（`log::trace!` / `log::debug!` / `log::info!` / `log::warn!`），target 为：

- `wireless`：顶层初始化
- `wireless::bsp`：命令管理、固件下载/启动
- `wireless::fdrv`：IPC、WiphyOps 占位实现

在 StarryOS 中由 axlog 实现 logger，需在 `main` 或 `starry_api::init` 中完成 `axlog::init()`。启用 wireless 调试输出示例：

```rust
axlog::init();
axlog::set_max_level("info");   // 只看 info 及以上
// 若需更细：
axlog::set_max_level("debug");  // 含 wireless::bsp / wireless::fdrv 的 debug
axlog::set_max_level("trace"); // 含 cmd_mgr wait_done 等 trace
```

按模块过滤需 axlog 支持按 target 过滤；若暂不支持，设置 `debug` 或 `trace` 即可看到所有 wireless 日志。
