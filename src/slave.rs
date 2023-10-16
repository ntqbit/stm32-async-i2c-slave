use core::marker::PhantomData;

use critical_section::CriticalSection;
use embassy_stm32::{
    gpio::{low_level::AFType, Pull},
    i2c::{self, SclPin, SdaPin},
    pac,
    time::Hertz,
    Peripheral,
};

pub use crate::{Control, Error, Event, Notification};

pub trait I2cBridge<T: i2c::Instance> {
    async fn receive(&self) -> Result<Event, Error>;

    fn write<'a>(&self, cs: CriticalSection, buf: &'a [u8]) -> &'a [u8];

    fn read(&self, cs: CriticalSection, buf: &mut [u8]) -> Result<usize, usize>;
}

pub struct I2CSlave<'d, T: i2c::Instance, B: I2cBridge<T>> {
    bridge: &'d B,
    _marker: PhantomData<T>,
}

impl<'d, T: i2c::Instance, B: I2cBridge<T>> I2CSlave<'d, T, B> {
    pub fn new(
        _i2c: impl Peripheral<P = T> + 'd,
        bridge: &'d B,
        scl: impl Peripheral<P = impl SclPin<T>> + 'd,
        sda: impl Peripheral<P = impl SdaPin<T>> + 'd,
        speed: Hertz,
        own_address: u8,
    ) -> Self {
        assert!(speed <= Hertz(100_000), "Fast-mode is not supported");
        assert!(
            own_address <= 127,
            "Own address is out of range. 10-bit addresses are not supported."
        );

        T::enable_and_reset();

        let scl = scl.into_ref();
        let sda = sda.into_ref();

        scl.set_as_af_pull(scl.af_num(), AFType::OutputOpenDrain, Pull::None);
        sda.set_as_af_pull(sda.af_num(), AFType::OutputOpenDrain, Pull::None);

        let clock_frequency = T::frequency();
        let freq = (clock_frequency.0 / 1_000_000) as u8;
        assert!(
            freq >= 2,
            "Bus frequency in Standard Mode must be at least 2MHz"
        );

        let regs = T::regs();

        regs.cr1().modify(|w| w.set_pe(false));

        regs.oar1().modify(|w| {
            w.set_addmode(pac::i2c::vals::Addmode::BIT7);
            w.set_add((own_address << 1) as u16);
        });

        regs.cr2().modify(|w| {
            w.set_itbufen(true);
            w.set_itevten(true);
            w.set_iterren(true);
            w.set_freq(freq);
        });

        regs.trise().modify(|w| w.set_trise(freq + 1));

        regs.ccr().modify(|w| {
            w.set_ccr((clock_frequency.0 / speed.0 / 2) as u16);
            w.set_duty(pac::i2c::vals::Duty::DUTY2_1);
            w.set_f_s(pac::i2c::vals::FS::STANDARD);
        });
        regs.cr1().modify(|w| {
            w.set_engc(true);
            w.set_ack(true);
            w.set_pe(true);
        });

        Self {
            bridge,
            _marker: PhantomData,
        }
    }

    pub fn write_cs<'a>(&self, cs: CriticalSection, buf: &'a [u8]) -> &'a [u8] {
        self.bridge.write(cs, buf)
    }

    pub fn write<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        critical_section::with(|cs| self.write_cs(cs, buf))
    }

    pub fn read(&self, cs: CriticalSection, buf: &mut [u8]) -> Result<usize, usize> {
        self.bridge.read(cs, buf)
    }

    pub fn n_read(&self, cs: CriticalSection) -> usize {
        self.bridge.read(cs, &mut []).unwrap_err()
    }

    pub async fn listen(&self) -> Result<Event, Error> {
        self.bridge.receive().await
    }
}
