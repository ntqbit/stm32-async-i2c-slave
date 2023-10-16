use critical_section::CriticalSection;
use embassy_stm32::i2c;

use super::{
    tx_lock::TxLockType, Control, Event, I2CError, Notification, ProtocolError, Reason, State,
};

pub trait InterruptBridge<T: i2c::Instance> {
    fn get_state(&self) -> State;

    fn set_state(&self, state: State);

    fn fail(&self, err: Reason);

    fn notify(&self, event: Event);

    fn lock_tx(&self, lock_type: TxLockType);

    fn unlock_tx(&self);

    fn get_rxbuf_size(&self, cs: CriticalSection) -> usize;

    fn write_rxbuf_byte(&self, cs: CriticalSection, byte: u8) -> Result<(), ()>;

    fn pop_txbuf_byte(&self, cs: CriticalSection) -> Option<u8>;

    fn reset_txbuf(&self, cs: CriticalSection) -> usize;
}

pub fn handle_event_interrupt<T: i2c::Instance, B: InterruptBridge<T>>(bridge: &B) {
    let regs = T::regs();
    let sr1 = regs.sr1().read();

    if sr1.txe() && sr1.rxne() {
        return bridge.fail(Reason::Protocol(ProtocolError::RxneAndTxne));
    }

    if sr1.addr() {
        match bridge.get_state() {
            state @ (State::Idle | State::Rx | State::Nack) => {
                let sr2 = regs.sr2().read();

                let transmission = sr2.tra();
                let general_call = sr2.gencall();

                bridge.set_state(if transmission {
                    State::TxInitial
                } else {
                    State::Rx
                });

                if matches!(state, State::Rx) {
                    bridge.notify(Event::Control(Control::Received {
                        size: critical_section::with(|cs| bridge.get_rxbuf_size(cs)),
                        write: transmission,
                    }));
                }

                bridge.notify(Event::Notification(Notification::Addr {
                    tx: transmission,
                    gencall: general_call,
                }));
            }
            State::TxInitial | State::TxRepeated => {
                return bridge.fail(Reason::Protocol(ProtocolError::AddrDuringTransmission))
            }
        }
    }

    if sr1.rxne() {
        match bridge.get_state() {
            State::Idle | State::TxInitial | State::TxRepeated | State::Nack => {
                return bridge.fail(Reason::Protocol(ProtocolError::RxneWhileNotReceiving))
            }
            State::Rx => {
                let byte = T::regs().dr().read().dr();
                let res = critical_section::with(|cs| bridge.write_rxbuf_byte(cs, byte));

                if res.is_err() {
                    return bridge.fail(Reason::ReceiveBufferFull);
                }
            }
        }
    }

    if sr1.txe() {
        match bridge.get_state() {
            State::Idle | State::Rx | State::Nack => {
                return bridge.fail(Reason::Protocol(ProtocolError::TxeWhileNotTranseiving))
            }
            state @ (State::TxInitial | State::TxRepeated) => {
                let initial = matches!(state, State::TxInitial);

                if initial || sr1.btf() {
                    let optbyte = critical_section::with(|cs| bridge.pop_txbuf_byte(cs));

                    if let Some(byte) = optbyte {
                        T::regs().dr().write(|w| w.set_dr(byte));

                        if initial {
                            bridge.set_state(State::TxRepeated);
                        }
                    } else {
                        bridge.lock_tx(TxLockType::TxAndBtf);
                        bridge.notify(Event::Control(Control::TxEmpty { initial }));
                    }
                } else {
                    // Waitinf for BTF.
                    bridge.lock_tx(TxLockType::TxOnly);
                }
            }
        }
    }

    if sr1.stopf() {
        regs.cr1().modify(|w| w.set_pe(true));

        match bridge.get_state() {
            state @ (State::Idle | State::Rx | State::Nack) => {
                if matches!(state, State::Rx) {
                    bridge.notify(Event::Control(Control::Received {
                        size: critical_section::with(|cs| bridge.get_rxbuf_size(cs)),
                        write: false,
                    }));
                }

                if !matches!(state, State::Idle) {
                    bridge.set_state(State::Idle);
                    bridge.notify(Event::Notification(Notification::Stop));
                }
            }
            State::TxInitial | State::TxRepeated => {
                return bridge.fail(Reason::Protocol(ProtocolError::StopDuringTransmission))
            }
        }
    }
}

pub fn handle_error_interrupt<T: i2c::Instance, B: InterruptBridge<T>>(bridge: &B) {
    let regs = T::regs();
    let sr1 = regs.sr1().read();

    if sr1.af() {
        regs.sr1().modify(|w| w.set_af(false));

        match bridge.get_state() {
            State::TxInitial | State::TxRepeated => {
                bridge.set_state(State::Nack);
                let sent = critical_section::with(|cs| bridge.reset_txbuf(cs));
                bridge.notify(Event::Notification(Notification::Sent { sent }));
            }
            State::Idle | State::Rx | State::Nack => {
                return bridge.fail(Reason::Protocol(ProtocolError::NackWhileNotTranseiving))
            }
        }
    }

    macro_rules! poly_set_error {
        ($name:ident NONE) => {};

        ($name:ident $err:ident) => {
            bridge.fail(Reason::I2C(I2CError::$err));
        };
    }

    macro_rules! clear_flag {
        ($name:ident NONE) => {};

        ($name:ident $set_func:ident) => {
            regs.sr1().modify(|w| w.$set_func(false));
        };
    }

    macro_rules! handle_errors {
        ([$(($name:ident, $set_func:ident, $err:ident)),*]) => {
            $(
                if sr1.$name() {
                    clear_flag!($name $set_func);
                    poly_set_error!($name $err);
                }
            )*
        };
    }

    handle_errors!([
        (berr, set_berr, NONE),
        (arlo, set_arlo, ArbitrationLoss),
        (ovr, set_ovr, Overrun),
        (pecerr, set_pecerr, PecError),
        (timeout, set_timeout, Timeout),
        (alert, set_alert, SmBusAlert)
    ]);
}
