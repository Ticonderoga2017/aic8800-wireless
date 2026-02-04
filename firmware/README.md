# WiFi 固件目录

固件按**名称**在此目录查找，与 LicheeRV 按路径/文件名加载一致。

## 使用方式

1. **本地嵌入（编译期）**  
   将以下文件从旧 wifi-driver 复制到本目录后，启用 BSP 的 `embed_firmware` feature 编译：
   - `fmacfw.bin`、`fmacfw_patch.bin`（AIC8801 必选）
   - 可选：`fw_adid.bin`、`fw_patch.bin`、`fw_patch_table.bin` 及 U03/DC/D80 等变体

2. **运行时注册**  
   不启用 `embed_firmware` 时，由平台在 `aicbsp_driver_fw_init` 前调用 `set_wifi_firmware(name, data)` 按名称注册多份固件（如 `fmacfw.bin`、`fmacfw_patch.bin`），BSP 通过 `get_firmware_by_name` 先查本地再查注册表。

## PHY 配置

- `rwnx_trident.ini` / `rwnx_karst.ini` 用于 PHY 校准，在固件启动后由 FDRV 通过 IPC（如 MM_SET_PHY_CFG）应用，与 wifi-driver 的 `apply_phy_cfg_from_ini` 对应。
