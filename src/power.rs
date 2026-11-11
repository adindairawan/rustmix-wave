//! AXP2101 support for panel power and sample-app battery status.
//!
//! The verified panel contract remains narrow: enable `ALDO3` before panel
//! initialization and disable it after deep sleep. The v0.4.0 sample-app slice
//! adds read-only battery / VBUS monitoring through reviewed register helpers.

use core::fmt::Debug;

use anyhow::{anyhow, bail, Result};
use embedded_hal::i2c::I2c;

use crate::power_key::{power_key_event_from_irq_status, PowerKeyEvent, POWER_KEY_EVENT_MASK};

const AXP2101_ADDRESS: u8 = 0x34;
const STATUS1: u8 = 0x00;
const STATUS2: u8 = 0x01;
const IC_TYPE: u8 = 0x03;
const AXP2101_CHIP_ID: u8 = 0x4A;
const PWRON_STATUS: u8 = 0x20;
const PWRON_SOURCE_VBUS_BIT: u8 = 1 << 0;
const PWRON_SOURCE_PWRON_PIN_BIT: u8 = 1 << 1;
const ADC_CHANNEL_CTRL: u8 = 0x30;
const ADC_BAT_VOLTAGE_HIGH: u8 = 0x34;
const ADC_BAT_VOLTAGE_LOW: u8 = 0x35;
const BAT_DET_CTRL: u8 = 0x68;
const LDO_ONOFF_CTRL0: u8 = 0x90;
const LDO_VOL2_CTRL: u8 = 0x94;
const BAT_PERCENT_DATA: u8 = 0xA4;
const INTEN2: u8 = 0x41;
const INTSTS2: u8 = 0x49;
const ALDO3_ENABLE_BIT: u8 = 1 << 2;
const ALDO3_MIN_MV: u16 = 500;
const ALDO3_MAX_MV: u16 = 3500;
const ALDO3_STEP_MV: u16 = 100;
const EPAPER_RAIL_MV: u16 = 3300;

/// Read-only AXP2101 status consumed by the sample-app UI shell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PowerSnapshot {
    /// Fuel-gauge percentage when a battery is detected.
    pub battery_percent: Option<u8>,
    /// Battery voltage in millivolts when a battery is detected.
    pub battery_voltage_mv: Option<u16>,
    /// USB VBUS is present and reported good by the PMIC.
    pub vbus_present: bool,
    /// PMIC charge state reports active charging.
    pub charging: bool,
}

/// Hardware power-on source reported by AXP2101 register `0x20` (`PWRON_STATUS`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PowerOnSource {
    pub raw: u8,
    pub vbus_insert: bool,
    pub power_key: bool,
}

/// Signals evaluated at boot to decide whether the device should stay in deep
/// sleep and silently charge instead of waking the e-paper UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BootSleepDecisionInput {
    pub persisted_sleep: bool,
    pub woke_by_ext_pin: bool,
    pub woke_by_timer: bool,
    pub reset_is_poweron: bool,
    pub vbus_present: bool,
    pub battery_connected: bool,
    pub power_on_source: PowerOnSource,
    pub pending_power_key: bool,
}

/// Return `true` when boot was triggered by USB/VBUS charging while the device
/// was turned off/asleep (without an intentional BOOT/PWR button or timer wake).
#[must_use]
pub const fn should_stay_asleep_on_boot(input: BootSleepDecisionInput) -> bool {
    if input.woke_by_ext_pin || input.woke_by_timer || input.pending_power_key {
        return false;
    }
    if !input.vbus_present || !input.battery_connected {
        return false;
    }
    if input.persisted_sleep {
        return true;
    }
    input.reset_is_poweron && input.power_on_source.vbus_insert && !input.power_on_source.power_key
}

/// Power contract consumed by the panel driver.
pub trait PanelPower {
    /// Configure and enable the panel rail.
    fn enable_panel_rail(&mut self) -> Result<()>;
    /// Disable the panel rail after e-paper deep sleep.
    fn disable_panel_rail(&mut self) -> Result<()>;
}

/// Narrow AXP2101 register-level driver.
pub struct Axp2101<I2C> {
    i2c: I2C,
    verified: bool,
}

impl<I2C> Axp2101<I2C> {
    #[must_use]
    pub fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            verified: false,
        }
    }
}

impl<I2C> Axp2101<I2C>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    fn read_register(&mut self, register: u8) -> Result<u8> {
        let mut value = [0_u8; 1];
        self.i2c
            .write_read(AXP2101_ADDRESS, &[register], &mut value)
            .map_err(|error| anyhow!("AXP2101 read 0x{register:02X} failed: {error:?}"))?;
        Ok(value[0])
    }

    fn write_register(&mut self, register: u8, value: u8) -> Result<()> {
        self.i2c
            .write(AXP2101_ADDRESS, &[register, value])
            .map_err(|error| anyhow!("AXP2101 write 0x{register:02X} failed: {error:?}"))
    }

    fn update_bits(&mut self, register: u8, mask: u8, enabled: bool) -> Result<()> {
        let value = self.read_register(register)?;
        let next = if enabled { value | mask } else { value & !mask };
        self.write_register(register, next)
    }

    fn verify_present(&mut self) -> Result<()> {
        if self.verified {
            return Ok(());
        }
        let chip_id = self.read_register(IC_TYPE)?;
        if chip_id != AXP2101_CHIP_ID {
            bail!(
                "unexpected PMIC chip ID: got 0x{chip_id:02X}, expected AXP2101 0x{AXP2101_CHIP_ID:02X}"
            );
        }
        self.verified = true;
        Ok(())
    }

    fn set_aldo3_voltage_mv(&mut self, millivolts: u16) -> Result<()> {
        let current = self.read_register(LDO_VOL2_CTRL)?;
        let encoded = encode_aldo3_voltage_mv(millivolts)?;
        self.write_register(LDO_VOL2_CTRL, (current & 0xE0) | encoded)
    }

    /// Enable only the PMIC measurement channels needed by the sample-app
    /// status slice. Charging policy remains untouched.
    pub fn initialize_sample_monitoring(&mut self) -> Result<()> {
        self.verify_present()?;
        self.update_bits(BAT_DET_CTRL, 1 << 0, true)?;
        self.update_bits(ADC_CHANNEL_CTRL, 1 << 0, true)
    }

    /// Enable AXP2101 short- and long-press Power-key reporting and clear any
    /// stale key status before the event loop starts.
    pub fn initialize_power_key_events(&mut self) -> Result<()> {
        self.verify_present()?;
        self.update_bits(INTEN2, POWER_KEY_EVENT_MASK, true)?;
        self.write_register(INTSTS2, POWER_KEY_EVENT_MASK)
    }

    /// Read and clear one latched AXP2101 Power-key event. Long press takes
    /// priority when both sticky bits are present.
    ///
    /// The status register is write-one-to-clear. Unrelated PMIC IRQ status bits
    /// are deliberately preserved for later isolated milestones.
    pub fn take_power_key_event(&mut self) -> Result<Option<PowerKeyEvent>> {
        self.verify_present()?;
        let status2 = self.read_register(INTSTS2)?;
        let event = power_key_event_from_irq_status(status2);
        let latched_key_bits = status2 & POWER_KEY_EVENT_MASK;
        if latched_key_bits != 0 {
            self.write_register(INTSTS2, latched_key_bits)?;
        }
        Ok(event)
    }

    /// Disable optional PMIC peripherals and clear latched power-key flags
    /// before MCU deep sleep while leaving system power rails and battery
    /// detection intact.
    pub fn prepare_for_deep_sleep(&mut self) -> Result<()> {
        self.update_bits(LDO_ONOFF_CTRL0, ALDO3_ENABLE_BIT, false)?;
        self.update_bits(ADC_CHANNEL_CTRL, 1 << 0, false)?;
        self.write_register(INTSTS2, POWER_KEY_EVENT_MASK)
    }

    /// Read the latched AXP2101 power-on source (`PWRON_STATUS`, register `0x20`).
    pub fn read_power_on_source(&mut self) -> Result<PowerOnSource> {
        self.verify_present()?;
        let raw = self.read_register(PWRON_STATUS)?;
        Ok(PowerOnSource {
            raw,
            vbus_insert: raw & PWRON_SOURCE_VBUS_BIT != 0,
            power_key: raw & PWRON_SOURCE_PWRON_PIN_BIT != 0,
        })
    }

    /// Read battery and VBUS state using the same AXP2101 register meanings as
    /// the uploaded sample application.
    pub fn read_power_snapshot(&mut self) -> Result<PowerSnapshot> {
        self.verify_present()?;
        let status1 = self.read_register(STATUS1)?;
        let status2 = self.read_register(STATUS2)?;
        let battery_connected = status1 & (1 << 3) != 0;

        let battery_percent = if battery_connected {
            Some(self.read_register(BAT_PERCENT_DATA)?.min(100))
        } else {
            None
        };
        let battery_voltage_mv = if battery_connected {
            let high = self.read_register(ADC_BAT_VOLTAGE_HIGH)?;
            let low = self.read_register(ADC_BAT_VOLTAGE_LOW)?;
            Some(decode_battery_voltage_mv(high, low))
        } else {
            None
        };

        Ok(PowerSnapshot {
            battery_percent,
            battery_voltage_mv,
            vbus_present: status1 & (1 << 5) != 0 && status2 & (1 << 3) == 0,
            charging: status2 >> 5 == 0x01,
        })
    }
}

impl<I2C> PanelPower for Axp2101<I2C>
where
    I2C: I2c,
    I2C::Error: Debug,
{
    fn enable_panel_rail(&mut self) -> Result<()> {
        self.verify_present()?;
        self.set_aldo3_voltage_mv(EPAPER_RAIL_MV)?;
        self.update_bits(LDO_ONOFF_CTRL0, ALDO3_ENABLE_BIT, true)
    }

    fn disable_panel_rail(&mut self) -> Result<()> {
        self.update_bits(LDO_ONOFF_CTRL0, ALDO3_ENABLE_BIT, false)
    }
}

fn decode_battery_voltage_mv(high: u8, low: u8) -> u16 {
    (u16::from(high & 0x1F) << 8) | u16::from(low)
}

fn encode_aldo3_voltage_mv(millivolts: u16) -> Result<u8> {
    if !(ALDO3_MIN_MV..=ALDO3_MAX_MV).contains(&millivolts) {
        bail!("ALDO3 voltage {millivolts} mV is outside the supported range");
    }
    if millivolts % ALDO3_STEP_MV != 0 {
        bail!("ALDO3 voltage {millivolts} mV must use {ALDO3_STEP_MV} mV steps");
    }

    Ok(((millivolts - ALDO3_MIN_MV) / ALDO3_STEP_MV) as u8)
}

#[cfg(test)]
mod tests {
    use super::{decode_battery_voltage_mv, encode_aldo3_voltage_mv};

    #[test]
    fn decodes_reference_battery_voltage_registers() {
        assert_eq!(decode_battery_voltage_mv(0x0F, 0x8C), 3_980);
    }

    #[test]
    fn encodes_epaper_rail_voltage() {
        assert_eq!(encode_aldo3_voltage_mv(3_300).unwrap(), 0x1C);
    }

    #[test]
    fn rejects_out_of_range_voltage() {
        assert!(encode_aldo3_voltage_mv(400).is_err());
        assert!(encode_aldo3_voltage_mv(3_600).is_err());
    }

    struct MockAxpI2c {
        regs: [u8; 256],
        writes: Vec<(u8, u8)>,
    }

    impl embedded_hal::i2c::ErrorType for MockAxpI2c {
        type Error = core::convert::Infallible;
    }

    impl embedded_hal::i2c::I2c for MockAxpI2c {
        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [embedded_hal::i2c::Operation<'_>],
        ) -> Result<(), Self::Error> {
            assert_eq!(address, super::AXP2101_ADDRESS);
            let mut active_reg = 0_usize;
            for op in operations {
                match op {
                    embedded_hal::i2c::Operation::Write(bytes) => {
                        if let Some((&reg, rest)) = bytes.split_first() {
                            active_reg = usize::from(reg);
                            for (offset, &val) in rest.iter().enumerate() {
                                let target = ((active_reg + offset) & 0xFF) as u8;
                                self.regs[usize::from(target)] = val;
                                self.writes.push((target, val));
                            }
                        }
                    }
                    embedded_hal::i2c::Operation::Read(buf) => {
                        for (offset, slot) in buf.iter_mut().enumerate() {
                            *slot = self.regs[(active_reg + offset) & 0xFF];
                        }
                    }
                }
            }
            Ok(())
        }
    }

    #[test]
    fn prepare_for_deep_sleep_disables_aldo3_and_battery_adc_and_clears_pek_irq() {
        let mut regs = [0_u8; 256];
        regs[usize::from(super::LDO_ONOFF_CTRL0)] = 0xFF;
        regs[usize::from(super::ADC_CHANNEL_CTRL)] = 0x05;
        let mut pmic = super::Axp2101::new(MockAxpI2c {
            regs,
            writes: Vec::new(),
        });
        pmic.prepare_for_deep_sleep().unwrap();
        assert_eq!(
            pmic.i2c.regs[usize::from(super::LDO_ONOFF_CTRL0)] & super::ALDO3_ENABLE_BIT,
            0
        );
        assert_eq!(pmic.i2c.regs[usize::from(super::ADC_CHANNEL_CTRL)], 0x04);
        assert!(pmic
            .i2c
            .writes
            .contains(&(super::INTSTS2, crate::power_key::POWER_KEY_EVENT_MASK)));
    }

    #[test]
    fn reads_power_on_source_bits_from_pwron_status_register() {
        let mut regs = [0_u8; 256];
        regs[usize::from(super::IC_TYPE)] = super::AXP2101_CHIP_ID;
        regs[usize::from(super::PWRON_STATUS)] = super::PWRON_SOURCE_VBUS_BIT;
        let mut pmic = super::Axp2101::new(MockAxpI2c {
            regs,
            writes: Vec::new(),
        });
        let source = pmic.read_power_on_source().unwrap();
        assert!(source.vbus_insert);
        assert!(!source.power_key);
    }

    #[test]
    fn stays_asleep_only_for_unrequested_vbus_charge_boots() {
        use super::{should_stay_asleep_on_boot, BootSleepDecisionInput, PowerOnSource};

        // 1. Sleeping + USB charger plugged in (no button wake) -> stay asleep
        assert!(should_stay_asleep_on_boot(BootSleepDecisionInput {
            persisted_sleep: true,
            vbus_present: true,
            battery_connected: true,
            ..BootSleepDecisionInput::default()
        }));

        // 2. Cold PMIC power-on purely from VBUS insertion with battery -> stay asleep
        assert!(should_stay_asleep_on_boot(BootSleepDecisionInput {
            reset_is_poweron: true,
            vbus_present: true,
            battery_connected: true,
            power_on_source: PowerOnSource {
                raw: 0x01,
                vbus_insert: true,
                power_key: false,
            },
            ..BootSleepDecisionInput::default()
        }));

        // 3. User pressed BOOT (EXT1) while charging -> wake up
        assert!(!should_stay_asleep_on_boot(BootSleepDecisionInput {
            persisted_sleep: true,
            woke_by_ext_pin: true,
            vbus_present: true,
            battery_connected: true,
            ..BootSleepDecisionInput::default()
        }));

        // 4. User pressed PWR key to turn on -> wake up
        assert!(!should_stay_asleep_on_boot(BootSleepDecisionInput {
            reset_is_poweron: true,
            vbus_present: true,
            battery_connected: true,
            power_on_source: PowerOnSource {
                raw: 0x03,
                vbus_insert: true,
                power_key: true,
            },
            ..BootSleepDecisionInput::default()
        }));

        // 5. USB powered without battery -> wake up
        assert!(!should_stay_asleep_on_boot(BootSleepDecisionInput {
            persisted_sleep: true,
            vbus_present: true,
            battery_connected: false,
            ..BootSleepDecisionInput::default()
        }));

        // 6. Normal awake reset while plugged into PC -> wake up
        assert!(!should_stay_asleep_on_boot(BootSleepDecisionInput {
            persisted_sleep: false,
            reset_is_poweron: false,
            vbus_present: true,
            battery_connected: true,
            ..BootSleepDecisionInput::default()
        }));
    }

    #[test]
    fn rejects_non_step_voltage() {
        assert!(encode_aldo3_voltage_mv(3_350).is_err());
    }
}
