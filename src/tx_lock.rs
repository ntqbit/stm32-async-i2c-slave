use core::marker::PhantomData;

use embassy_stm32::i2c;

pub enum TxLockType {
    TxOnly,
    TxAndBtf,
}

pub struct TxLock<T: i2c::Instance> {
    _marker: PhantomData<T>,
}

impl<T: i2c::Instance> TxLock<T> {
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    pub fn lock(&self, lock_type: TxLockType) {
        T::regs().cr2().modify(|w| {
            w.set_itbufen(false);
            w.set_itevten(matches!(lock_type, TxLockType::TxOnly));
        });
    }

    pub fn unlock(&self) {
        T::regs().cr2().modify(|w| {
            w.set_itbufen(true);
            w.set_itevten(true);
        });
    }
}
