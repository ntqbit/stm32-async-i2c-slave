#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(async_fn_in_trait)]

use cortex_m::peripheral::NVIC;

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::{self as _, interrupt, time::Hertz};
use panic_probe as _;
use stm32_async_i2c_slave::{handle_error_interrupt, handle_event_interrupt, Bridge, I2CSlave};

static I2C_BRIDGE: Bridge<embassy_stm32::peripherals::I2C1, 32, 32, 32> = Bridge::new();

#[interrupt]
#[allow(non_snake_case)]
fn I2C1_EV() {
    handle_event_interrupt(&I2C_BRIDGE);
}

#[interrupt]
#[allow(non_snake_case)]
fn I2C1_ER() {
    handle_error_interrupt(&I2C_BRIDGE);
}

#[derive(defmt::Format, Debug, Clone, Copy, num_enum::TryFromPrimitive)]
#[allow(non_camel_case_types)]
#[repr(u8)]
enum Bme280Registers {
    BME280_REGISTER_DIG_T1 = 0x88,
    BME280_REGISTER_DIG_T2 = 0x8A,
    BME280_REGISTER_DIG_T3 = 0x8C,

    BME280_REGISTER_DIG_P1 = 0x8E,
    BME280_REGISTER_DIG_P2 = 0x90,
    BME280_REGISTER_DIG_P3 = 0x92,
    BME280_REGISTER_DIG_P4 = 0x94,
    BME280_REGISTER_DIG_P5 = 0x96,
    BME280_REGISTER_DIG_P6 = 0x98,
    BME280_REGISTER_DIG_P7 = 0x9A,
    BME280_REGISTER_DIG_P8 = 0x9C,
    BME280_REGISTER_DIG_P9 = 0x9E,

    BME280_REGISTER_DIG_H1 = 0xA1,
    BME280_REGISTER_DIG_H2 = 0xE1,
    BME280_REGISTER_DIG_H3 = 0xE3,
    BME280_REGISTER_DIG_H4 = 0xE4,
    BME280_REGISTER_DIG_H5 = 0xE5,
    BME280_REGISTER_DIG_H5_2 = 0xE6,
    BME280_REGISTER_DIG_H6 = 0xE7,

    BME280_REGISTER_CHIPID = 0xD0,
    BME280_REGISTER_RESET = 0xE0,

    BME280_REGISTER_CONTROLHUMID = 0xF2,
    BME280_REGISTER_STATUS = 0xF3,
    BME280_REGISTER_CONTROL = 0xF4,
    BME280_REGISTER_CONFIG = 0xF5,
    BME280_REGISTER_MEASUREMENTS = 0xF7,
    BME280_REGISTER_TEMPDATA = 0xFA,
    BME280_REGISTER_HUMIDDATA = 0xFD,
}

#[allow(dead_code)]
const BME280_MODE_FORCED: u8 = 0b01;

#[derive(defmt::Format, Clone, Copy, num_enum::TryFromPrimitive)]
#[allow(non_camel_case_types)]
#[repr(u8)]
enum Bme280ResetType {
    BME280_SOFT_RESET = 0xB6,
}

#[allow(dead_code)]
const BME280_STATUS_IM_UPDATE: u8 = 0b01;

#[embassy_executor::main]
async fn main_task(_spawner: Spawner) {
    defmt::info!("Start!");

    let peripherals = embassy_stm32::init(Default::default());

    let slave = I2CSlave::new(
        peripherals.I2C1,
        &I2C_BRIDGE,
        peripherals.PB6,
        peripherals.PB7,
        Hertz(100_000),
        0x76,
    );

    unsafe {
        NVIC::unmask(interrupt::I2C1_ER);
        NVIC::unmask(interrupt::I2C1_EV);
    }

    let mut last_reading_reg: Option<Bme280Registers> = None;
    let mut reading_reg: Option<Bme280Registers> = None;
    let mut buf = [0u8; 2];

    loop {
        use stm32_async_i2c_slave::{Control, Event};

        match slave.listen().await {
            Ok(Event::Notification(n)) => defmt::info!("Notification: {}", n),
            Ok(Event::Control(ctl)) => match ctl {
                Control::Received { .. } => {
                    let size = critical_section::with(|cs| slave.read(cs, &mut buf).unwrap());
                    let buf = &buf[..size];
                    defmt::info!("Received: {}", buf);

                    match size {
                        0 => {}
                        1 => {
                            reading_reg.replace(
                                Bme280Registers::try_from(buf[0]).expect("Unknown register number"),
                            );
                        }
                        2 => {
                            let regnum = buf[0];
                            if let Ok(reg) = Bme280Registers::try_from(regnum) {
                                let val = buf[1];

                                match reg {
                                    Bme280Registers::BME280_REGISTER_RESET => {
                                        defmt::info!(
                                            "Chip reset: {}",
                                            Bme280ResetType::try_from(val).map_err(|_| ())
                                        )
                                    }
                                    Bme280Registers::BME280_REGISTER_CONTROL => {
                                        defmt::info!("Control: {:X}", val);
                                    }
                                    Bme280Registers::BME280_REGISTER_CONTROLHUMID => {
                                        defmt::info!("ControlHumid: {:X}", val)
                                    }
                                    Bme280Registers::BME280_REGISTER_CONFIG => {
                                        defmt::info!("Config: {:X}", val)
                                    }
                                    _ => panic!("Unexpected register write: {:?}", reg),
                                }
                            } else {
                                panic!("Unknown register number: {}", regnum);
                            }
                        }
                        _ => panic!("Unexpected"),
                    }
                }
                ev @ Control::TxEmpty { initial, .. } => {
                    let reg = if initial {
                        last_reading_reg = reading_reg;
                        reading_reg.take()
                    } else {
                        last_reading_reg
                    };
                    let reg = reg.expect("No reading reg set.");
                    defmt::info!("Reg: {}. Ev: {}", reg, ev);

                    let buf: &[u8] = match reg {
                        Bme280Registers::BME280_REGISTER_CHIPID => &[0x60],
                        Bme280Registers::BME280_REGISTER_STATUS => &[0x00],
                        Bme280Registers::BME280_REGISTER_DIG_T1 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_T2 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_T3 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P1 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P2 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P3 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P4 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P5 => &[0x60, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P6 => &[0x00, 0x60],
                        Bme280Registers::BME280_REGISTER_DIG_P7 => &[0x00, 0x00],
                        Bme280Registers::BME280_REGISTER_DIG_P8 => &[0x00, 0x00],
                        Bme280Registers::BME280_REGISTER_DIG_P9 => &[0x00, 0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H1 => &[0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H2 => &[0x00, 0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H3 => &[0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H4 => &[0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H5 => &[0x00, 0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H5_2 => &[0x00],
                        Bme280Registers::BME280_REGISTER_DIG_H6 => &[0x00],
                        Bme280Registers::BME280_REGISTER_CONTROLHUMID => &[0x00],
                        Bme280Registers::BME280_REGISTER_CONFIG => &[0x00],
                        Bme280Registers::BME280_REGISTER_MEASUREMENTS => &[0x90],
                        _ => panic!("Unexpected register read: {:?}", reg),
                    };

                    defmt::info!("Writing {} bytes.", buf.len());
                    slave.write(buf);
                }
            },
            Err(fail) => {
                defmt::error!("Fail: {}", fail);
                break;
            }
        }
    }
}
