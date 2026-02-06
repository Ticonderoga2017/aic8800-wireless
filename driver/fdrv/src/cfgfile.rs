//! rwnx 配置文件解析（与 LicheeRV rwnx_cfgfile.c 对齐）
//!
//! 解析 rwnx_karst.ini / rwnx_trident.ini 等：MAC_ADDR=、KARST_*、TRD_* 等 tag，
//! 供启动时下发给固件或设置本地 MAC。

use core::result::Result;

/// 解析后的配置（与 rwnx_conf_file / rwnx_phy_conf_file 语义对齐）
#[derive(Debug, Clone, Default)]
pub struct RwnxConfFile {
    pub mac_addr: [u8; 6],
}

/// Karst 物理层配置（rwnx_karst.ini 中 KARST_*）
#[derive(Debug, Clone, Default)]
pub struct RwnxKarstConf {
    pub tx_iq_comp_2_4g_path_0: u32,
    pub tx_iq_comp_2_4g_path_1: u32,
    pub rx_iq_comp_2_4g_path_0: u32,
    pub rx_iq_comp_2_4g_path_1: u32,
    pub tx_iq_comp_5g_path_0: u32,
    pub tx_iq_comp_5g_path_1: u32,
    pub rx_iq_comp_5g_path_0: u32,
    pub rx_iq_comp_5g_path_1: u32,
    pub default_path: u8,
}

/// 在 file_data 中查找 tag_name= 开头的行，返回等号后的值起始指针（不含换行）
/// 与 LicheeRV rwnx_find_tag 一致
fn find_tag<'a>(file_data: &'a [u8], tag_name: &str) -> Option<&'a [u8]> {
    let tag = tag_name.as_bytes();
    let mut line_start = 0;
    while line_start < file_data.len() {
        let mut curr = line_start;
        while curr < file_data.len() && file_data[curr] != b'\n' {
            curr += 1;
        }
        let line = &file_data[line_start..curr];
        if line.len() >= tag.len() && &line[..tag.len()] == tag {
            let value_start = line_start + tag.len();
            return Some(&file_data[value_start..curr]);
        }
        line_start = curr + 1;
    }
    None
}

/// 解析 MAC_ADDR=00:00:00:00:00:00 格式
fn parse_mac_addr(s: &[u8]) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut i = 0;
    let mut byte_idx = 0;
    while byte_idx < 6 && i + 2 <= s.len() {
        let hi = hex_nibble(s[i])?;
        let lo = hex_nibble(s[i + 1])?;
        out[byte_idx] = (hi << 4) | lo;
        byte_idx += 1;
        i += 2;
        if byte_idx < 6 && i < s.len() && s[i] == b':' {
            i += 1;
        }
    }
    if byte_idx == 6 {
        Some(out)
    } else {
        None
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 解析 8 位十六进制为 u32
fn parse_hex8(s: &[u8]) -> Option<u32> {
    if s.len() < 8 {
        return None;
    }
    let mut v: u32 = 0;
    for i in 0..8 {
        v = (v << 4) | hex_nibble(s.get(i).copied()?)? as u32;
    }
    Some(v)
}

/// 解析配置文件（与 rwnx_parse_configfile 一致）：MAC_ADDR=
pub fn parse_configfile(file_data: &[u8], config: &mut RwnxConfFile) -> Result<(), i32> {
    const DEFAULT_MAC: [u8; 6] = [0, 111, 111, 111, 111, 0];
    if let Some(value) = find_tag(file_data, "MAC_ADDR=") {
        if let Some(mac) = parse_mac_addr(value) {
            config.mac_addr = mac;
            return Ok(());
        }
    }
    config.mac_addr = DEFAULT_MAC;
    Ok(())
}

/// 解析 rwnx_karst.ini（与 rwnx_parse_phy_configfile 中 Karst 部分一致）
pub fn parse_karst_configfile(file_data: &[u8], config: &mut RwnxKarstConf) -> Result<(), i32> {
    const DEFAULT_IQ: u32 = 0x0100_0000;
    macro_rules! parse_tag {
        ($tag:expr, $field:ident, $default:expr) => {
            if let Some(v) = find_tag(file_data, $tag) {
                config.$field = parse_hex8(v).unwrap_or($default);
            } else {
                config.$field = $default;
            }
        };
    }
    parse_tag!("KARST_TX_IQ_COMP_2_4G_PATH_0=", tx_iq_comp_2_4g_path_0, DEFAULT_IQ);
    parse_tag!("KARST_TX_IQ_COMP_2_4G_PATH_1=", tx_iq_comp_2_4g_path_1, DEFAULT_IQ);
    parse_tag!("KARST_RX_IQ_COMP_2_4G_PATH_0=", rx_iq_comp_2_4g_path_0, DEFAULT_IQ);
    parse_tag!("KARST_RX_IQ_COMP_2_4G_PATH_1=", rx_iq_comp_2_4g_path_1, DEFAULT_IQ);
    parse_tag!("KARST_TX_IQ_COMP_5G_PATH_0=", tx_iq_comp_5g_path_0, DEFAULT_IQ);
    parse_tag!("KARST_TX_IQ_COMP_5G_PATH_1=", tx_iq_comp_5g_path_1, DEFAULT_IQ);
    parse_tag!("KARST_RX_IQ_COMP_5G_PATH_0=", rx_iq_comp_5g_path_0, DEFAULT_IQ);
    parse_tag!("KARST_RX_IQ_COMP_5G_PATH_1=", rx_iq_comp_5g_path_1, DEFAULT_IQ);
    if let Some(v) = find_tag(file_data, "KARST_DEFAULT_PATH=") {
        if !v.is_empty() {
            config.default_path = hex_nibble(v[0]).unwrap_or(2);
        }
    }
    Ok(())
}
