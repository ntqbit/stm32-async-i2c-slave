#![no_std]
#![feature(async_fn_in_trait)]

mod bridge;
mod interrupts;
mod receive_buffer;
mod send_buffer;
mod slave;
mod state_holder;
mod tx_lock;

pub use bridge::Bridge;
pub use interrupts::{handle_error_interrupt, handle_event_interrupt};
pub use slave::{I2CSlave, I2cBridge};

#[cfg(feature = "dump")]
use bridge::StateDump;

#[derive(Debug, Clone, Copy, bytemuck::NoUninit)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum State {
    Idle,
    TxInitial,
    TxRepeated,
    Rx,
    Nack,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Notification {
    Addr { tx: bool, gencall: bool },
    Sent { sent: usize },
    Stop,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Control {
    Received { size: usize, write: bool },
    TxEmpty { initial: bool },
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event {
    Notification(Notification),
    Control(Control),
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum I2CError {
    #[allow(dead_code)]
    BusError,
    ArbitrationLoss,
    #[allow(dead_code)]
    AcknowledgeFailure,
    Overrun,
    PecError,
    Timeout,
    SmBusAlert,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ProtocolError {
    RxneAndTxne,
    AddrDuringTransmission,
    RxneWhileNotReceiving,
    TxeWhileNotTranseiving,
    StopDuringTransmission,
    NackWhileNotTranseiving,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Reason {
    I2C(I2CError),
    Protocol(ProtocolError),
    ReceiveBufferFull,
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Error {
    #[cfg(feature = "dump")]
    pub dump: StateDump,
    pub reason: Reason,
}
