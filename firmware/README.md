# WiFi 固件目录

固件按**名称**在此目录查找，与 LicheeRV 按路径/文件名加载一致。

## 固件来源

本目录中的 `.bin` 固件已从**旧版 wifi-driver** 全部拷贝而来，来源路径：

- `wireless-fail-bak/wifi-driver/firmware/`

当前包含的固件文件：

| 类型 | 文件名 | 说明 |
|------|--------|------|
| AIC8801 | `fmacfw.bin` | 主 WiFi 固件（U02/U03 工作模式） |
| | `fmacfw_patch.bin` | 8801 patch |
| | `fw_adid.bin`, `fw_patch.bin`, `fw_patch_table.bin` | BT 相关（U02） |
| | `fw_adid_u03.bin`, `fw_patch_u03.bin`, `fw_patch_table_u03.bin` | BT 相关（U03） |
| AIC8800DC | `fmacfw_patch_8800dc_u02.bin` | 8800DC U02 主固件 |
| | `fw_adid_8800dc_u02.bin`, `fw_patch_8800dc_u02.bin`, `fw_patch_table_8800dc_u02.bin` | 8800DC BT/patch |
| AIC8800D80 | `fmacfw_8800d80_u02.bin` | 8800D80 主固件 |
| | `fw_adid_8800d80_u02.bin`, `fw_patch_8800d80_u02.bin`, `fw_patch_table_8800d80_u02.bin` | 8800D80 BT/patch |

**说明**：旧版 wifi-driver 中未包含 `fmacfw_rf.bin`。若需启用 **RF 测试模式**（Test 模式），请从 LicheeRV 或 AIC 官方固件包中取得 `fmacfw_rf.bin` 并放入本目录，否则嵌入固件时 RF 模式将无法使用。

## 使用方式

1. **本地嵌入（编译期，默认）**  
   BSP 默认启用 `embed_firmware_8801`，仅嵌入 **8801 必需** 的两份：`fmacfw.bin`、`fmacfw_patch.bin`。  
   请将这两份从旧版 wifi-driver 或 LicheeRV 固件目录复制到本目录 `wireless/firmware/`，否则编译会报 “No such file or directory”。  
   启用 `embed_firmware` 可嵌入全部 `.bin`（需本目录下所有对应文件存在）。

2. **运行时注册**  
   编译时关闭 `embed_firmware_8801`（在依赖 bsp 处设置 `default-features = false`）时，由平台在 `aicbsp_driver_fw_init` 前调用 `set_wifi_firmware(name, data)` 按名称注册固件，BSP 通过 `get_firmware_by_name` 先查本地再查注册表。

## PHY 配置

- `rwnx_trident.ini` / `rwnx_karst.ini` 用于 PHY 校准，在固件启动后由 FDRV 通过 IPC（如 MM_SET_PHY_CFG）应用，与 wifi-driver 的 `apply_phy_cfg_from_ini` 对应。
