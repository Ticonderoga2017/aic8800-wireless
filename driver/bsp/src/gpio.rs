//! SG2002 GPIO控制模块
//! 
//! 用于控制WiFi芯片的复位和电源引脚

use axhal::mem::{pa, phys_to_virt};
use axerrno::{AxError, AxResult};
use core::ptr::{read_volatile, write_volatile};

use crate::sync;

/// GPIO控制器基地址
/// 参考SG2002技术手册，GPIO0基地址
const GPIO0_BASE: usize = 0x03020000;
const GPIO1_BASE: usize = 0x03021000;
const GPIO2_BASE: usize = 0x03022000;
const GPIO3_BASE: usize = 0x03023000;

/// GPIO寄存器偏移
mod gpio_regs {
    /// 数据寄存器（读取/写入GPIO值）
    pub const SWPORTA_DR: usize = 0x00;
    /// 数据方向寄存器（0=输入，1=输出）
    pub const SWPORTA_DDR: usize = 0x04;
    /// 中断使能寄存器
    pub const INTEN: usize = 0x30;
    /// 中断屏蔽寄存器
    pub const INTMASK: usize = 0x34;
    /// 中断类型寄存器
    pub const INTTYPE_LEVEL: usize = 0x38;
    /// 中断极性寄存器
    pub const INT_POLARITY: usize = 0x3C;
    /// 中断状态寄存器
    pub const INTSTATUS: usize = 0x40;
    /// 原始中断状态寄存器
    pub const RAW_INTSTATUS: usize = 0x44;
}

/// GPIO引脚定义
#[derive(Debug, Clone, Copy)]
pub struct GpioPin {
    /// GPIO控制器编号（0-3）
    pub controller: u8,
    /// 引脚编号（0-31）
    pub pin: u8,
}

impl GpioPin {
    pub const fn new(controller: u8, pin: u8) -> Self {
        Self { controller, pin }
    }
}

/// WiFi 芯片（AIC8800）GPIO 引脚配置
///
/// **LicheeRV Nano W 配置（来自 U-Boot cvi_board_init.c）**：
/// - WiFi 电源/复位引脚：**GPIOA_26** (controller=0, pin=26)
///   - pinmux: 0x0300104C = 0x3 (GPIO 模式)
///   - 低电平 50ms → 高电平（上电序列）
/// - SD1 pinmux（Active 域 0x03001000 + offset）：
///   - 0x030010D0 = 0 (D3), 0x030010D4 = 0 (D2), 0x030010D8 = 0 (D1)
///   - 0x030010DC = 0 (D0), 0x030010E0 = 0 (CMD), 0x030010E4 = 0 (CLK)
pub mod wifi_pins {
    use super::GpioPin;

    /// WiFi 芯片电源/复位引脚（LicheeRV Nano W：GPIOA_26）
    /// 对应 U-Boot: mmio_write_32(0x0300104C, 0x3) + GPIOA bit 26
    pub const WIFI_POWER_EN: GpioPin = GpioPin::new(0, 26);

    /// WiFi 芯片复位引脚（与电源共用同一引脚，AIC8800 只需一个控制引脚）
    pub const WIFI_RESET: GpioPin = GpioPin::new(0, 26);

    /// WiFi 芯片唤醒引脚（可选；LicheeRV Nano W 未使用）
    pub const WIFI_WAKE: Option<GpioPin> = None;
}

/// GPIO控制器
pub struct GpioController {
    /// 控制器虚拟地址（已转换）
    base_vaddr: usize,
}

impl GpioController {
    /// 创建GPIO控制器
    pub fn new(controller: u8) -> AxResult<Self> {
        let paddr = match controller {
            0 => GPIO0_BASE,
            1 => GPIO1_BASE,
            2 => GPIO2_BASE,
            3 => GPIO3_BASE,
            _ => return Err(AxError::InvalidInput),
        };
        
        // 将物理地址转换为虚拟地址
        let paddr_phys = pa!(paddr);
        let vaddr = phys_to_virt(paddr_phys);
        
        log::debug!("GPIO{}物理地址: {:#x}, 虚拟地址: {:#x}", 
                   controller, paddr, vaddr.as_usize());
        
        Ok(Self {
            base_vaddr: vaddr.as_usize(),
        })
    }
    
    /// 读取寄存器
    unsafe fn read_reg(&self, offset: usize) -> u32 {
        let addr = self.base_vaddr + offset;
        read_volatile(addr as *const u32)
    }
    
    /// 写入寄存器
    unsafe fn write_reg(&self, offset: usize, value: u32) {
        let addr = self.base_vaddr + offset;
        write_volatile(addr as *mut u32, value);
    }
    
    /// 设置引脚为输出模式
    pub fn set_output(&mut self, pin: u8) -> AxResult<()> {
        if pin >= 32 {
            return Err(AxError::InvalidInput);
        }
        
        unsafe {
            let ddr = self.read_reg(gpio_regs::SWPORTA_DDR);
            self.write_reg(gpio_regs::SWPORTA_DDR, ddr | (1 << pin));
        }
        
        Ok(())
    }
    
    /// 设置引脚为输入模式
    pub fn set_input(&mut self, pin: u8) -> AxResult<()> {
        if pin >= 32 {
            return Err(AxError::InvalidInput);
        }
        
        unsafe {
            let ddr = self.read_reg(gpio_regs::SWPORTA_DDR);
            self.write_reg(gpio_regs::SWPORTA_DDR, ddr & !(1 << pin));
        }
        
        Ok(())
    }
    
    /// 设置引脚电平（高/低）
    pub fn set_pin(&mut self, pin: u8, high: bool) -> AxResult<()> {
        if pin >= 32 {
            return Err(AxError::InvalidInput);
        }
        
        unsafe {
            let dr = self.read_reg(gpio_regs::SWPORTA_DR);
            if high {
                self.write_reg(gpio_regs::SWPORTA_DR, dr | (1 << pin));
            } else {
                self.write_reg(gpio_regs::SWPORTA_DR, dr & !(1 << pin));
            }
        }
        
        Ok(())
    }
    
    /// 读取引脚电平
    pub fn get_pin(&self, pin: u8) -> AxResult<bool> {
        if pin >= 32 {
            return Err(AxError::InvalidInput);
        }
        
        unsafe {
            let dr = self.read_reg(gpio_regs::SWPORTA_DR);
            Ok((dr & (1 << pin)) != 0)
        }
    }
}

/// WiFi 芯片电源控制（LicheeRV Nano W：单引脚 GPIOA_26 控制）
pub struct WifiGpioControl {
    gpio: GpioController,
    pin: GpioPin,
}

impl WifiGpioControl {
    /// 创建 WiFi GPIO 控制实例（LicheeRV Nano W 使用 GPIOA_26）
    pub fn new() -> AxResult<Self> {
        let pin = wifi_pins::WIFI_POWER_EN;
        let gpio = GpioController::new(pin.controller)?;
        
        Ok(Self { gpio, pin })
    }
    
    /// 初始化 GPIO（设置为输出模式）
    pub fn init(&mut self) -> AxResult<()> {
        log::info!("初始化 WiFi GPIO 控制: GPIO{}_{} (controller={}, pin={})", 
                   self.pin.controller, self.pin.pin, self.pin.controller, self.pin.pin);
        self.gpio.set_output(self.pin.pin)?;
        Ok(())
    }
    
    /// WiFi 芯片上电序列（与 LicheeRV Allwinner/Rockchip2、U-Boot cvi_board_init.c 对齐）
    ///
    /// LicheeRV：power(0)→mdelay(50)→power(1)→mdelay(50)。旧 wifi-driver 为两引脚：先 power 再 reset。
    /// 本板单引脚 GPIOA_26：低 50ms → 高 50ms。
    pub fn power_on(&mut self) -> AxResult<()> {
        log::info!("WiFi 上电序列: 拉低 → 50ms → 拉高 (LicheeRV 50/50)");
        self.gpio.set_pin(self.pin.pin, false)?;
        sync::delay_spin_ms(50);
        self.gpio.set_pin(self.pin.pin, true)?;
        sync::delay_spin_ms(50);
        log::info!("WiFi 上电完成: GPIO{}_{} = HIGH", self.pin.controller, self.pin.pin);
        Ok(())
    }
    
    /// 复位序列（与上电共用同一引脚，单引脚设计下与 power_on 一致）
    pub fn reset(&mut self) -> AxResult<()> {
        self.power_on()
    }
    
    /// 完整上电+复位（单引脚：一次 power_on 即完成；调用方在 aicbsp_power_on 内会再做 50ms+POST_POWER_STABLE_MS 再 sdio_init）
    pub fn power_on_and_reset(&mut self) -> AxResult<()> {
        self.power_on()
    }

    /// 读回当前引脚电平
    pub fn readback_state(&self) -> AxResult<(bool, bool)> {
        let high = self.gpio.get_pin(self.pin.pin)?;
        Ok((high, high))  // 电源和复位是同一引脚
    }

    /// 验证上电状态
    pub fn verify_after_power_on(&mut self) -> AxResult<bool> {
        let high = self.gpio.get_pin(self.pin.pin)?;
        log::info!("WiFi GPIO 验证: GPIO{}_{}={} => {}", 
                   self.pin.controller, self.pin.pin, high, if high { "OK" } else { "FAIL" });
        Ok(high)
    }
    
    /// 关闭 WiFi 电源
    pub fn power_off(&mut self) -> AxResult<()> {
        log::info!("关闭 WiFi 电源");
        self.gpio.set_pin(self.pin.pin, false)?;
        sync::delay_spin_ms(100);
        Ok(())
    }
}
