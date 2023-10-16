use core::cell::RefCell;

use critical_section::{CriticalSection, Mutex};
use embassy_stm32::i2c;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel};
use heapless::Deque;

use super::{
    interrupts::InterruptBridge,
    receive_buffer::ReceiveBuffer,
    send_buffer::SendBuffer,
    slave::I2cBridge,
    state_holder::StateHolder,
    tx_lock::{TxLock, TxLockType},
    Error, Event, Notification, Reason, State,
};

pub const STATES_HISTORY_SIZE: usize = 5;
pub const EVENTS_HISTORY_SIZE: usize = 5;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct StateDump {
    pub state_history: [State; STATES_HISTORY_SIZE],
    pub current_state: State,
    pub event_history: [Event; EVENTS_HISTORY_SIZE],
}

pub type InterruptChannel<const SZ: usize> =
    channel::Channel<CriticalSectionRawMutex, Result<Event, Error>, SZ>;

pub struct Bridge<
    T: i2c::Instance,
    const CHSIZE: usize,
    const TXBUFSIZE: usize,
    const RXBUFSIZE: usize,
> {
    channel: InterruptChannel<CHSIZE>,

    tx_lock: TxLock<T>,
    send_buffer: Mutex<RefCell<SendBuffer<TXBUFSIZE>>>,

    receive_buffer: Mutex<RefCell<ReceiveBuffer<RXBUFSIZE>>>,

    state_holder: StateHolder<STATES_HISTORY_SIZE>,
    events_history: Mutex<RefCell<Deque<Event, EVENTS_HISTORY_SIZE>>>,
}

#[cfg(feature = "dump")]
fn deque_into_array<T: Copy, const N: usize>(d: &Deque<T, N>, arr: &mut [T; N]) {
    let n = d.len();
    let (a, b) = d.as_slices();
    let s = N - n;

    arr[s..s + a.len()].copy_from_slice(a);
    arr[s + a.len()..].copy_from_slice(b);
}

impl<T: i2c::Instance, const CHSIZE: usize, const TXBUFSIZE: usize, const RXBUFSIZE: usize>
    Bridge<T, CHSIZE, TXBUFSIZE, RXBUFSIZE>
{
    pub const fn new() -> Self {
        Self {
            channel: InterruptChannel::new(),
            tx_lock: TxLock::new(),
            send_buffer: Mutex::new(RefCell::new(SendBuffer::new())),
            receive_buffer: Mutex::new(RefCell::new(ReceiveBuffer::new())),
            state_holder: StateHolder::new(),
            events_history: Mutex::new(RefCell::new(Deque::new())),
        }
    }

    fn disable_peripheral() {
        T::regs().cr1().modify(|w| w.set_pe(false));
    }

    fn send_channel(&self, result: Result<Event, Error>) {
        self.channel.try_send(result).expect("Channel is full")
    }

    #[cfg(feature = "dump")]
    pub fn dump_state(&self) -> StateDump {
        let mut states = [State::Idle; STATES_HISTORY_SIZE];
        let mut events: [Event; EVENTS_HISTORY_SIZE] =
            [Event::Notification(Notification::Stop); EVENTS_HISTORY_SIZE];

        critical_section::with(|cs| {
            let states_deque = self.state_holder.get_history(cs);
            deque_into_array(&states_deque, &mut states);

            let events_deque = self.events_history.borrow_ref(cs);
            deque_into_array(&events_deque, &mut events);
        });

        StateDump {
            state_history: states,
            current_state: self.get_state(),
            event_history: events,
        }
    }
}

impl<T: i2c::Instance, const CHSIZE: usize, const TXBUFSIZE: usize, const RXBUFSIZE: usize>
    I2cBridge<T> for Bridge<T, CHSIZE, TXBUFSIZE, RXBUFSIZE>
{
    async fn receive(&self) -> Result<Event, Error> {
        self.channel.receive().await
    }

    fn write<'a>(&self, cs: CriticalSection, buf: &'a [u8]) -> &'a [u8] {
        let res = self.send_buffer.borrow_ref_mut(cs).write(buf);
        self.unlock_tx();
        res
    }

    fn read(&self, cs: CriticalSection, buf: &mut [u8]) -> Result<usize, usize> {
        let mut rb = self.receive_buffer.borrow_ref_mut(cs);
        let r = rb.read(buf);

        if r.is_ok() {
            rb.reset();
        }

        r
    }
}

impl<T: i2c::Instance, const CHSIZE: usize, const TXBUFSIZE: usize, const RXBUFSIZE: usize>
    InterruptBridge<T> for Bridge<T, CHSIZE, TXBUFSIZE, RXBUFSIZE>
{
    fn get_state(&self) -> State {
        self.state_holder.get_state()
    }

    fn set_state(&self, state: State) {
        self.state_holder.set_state(state)
    }

    fn fail(&self, reason: Reason) {
        Self::disable_peripheral();

        self.send_channel(Err(Error {
            #[cfg(feature = "dump")]
            dump: self.dump_state(),
            reason,
        }));
    }

    fn notify(&self, event: Event) {
        critical_section::with(|cs| {
            let mut h = self.events_history.borrow_ref_mut(cs);
            if h.is_full() {
                h.pop_front();
            }
            h.push_back(event).unwrap();
        });

        self.send_channel(Ok(event))
    }

    fn lock_tx(&self, lock_type: TxLockType) {
        self.tx_lock.lock(lock_type)
    }

    fn unlock_tx(&self) {
        self.tx_lock.unlock()
    }

    fn get_rxbuf_size(&self, cs: CriticalSection) -> usize {
        self.receive_buffer.borrow_ref(cs).get_size()
    }

    fn write_rxbuf_byte(&self, cs: CriticalSection, byte: u8) -> Result<(), ()> {
        self.receive_buffer.borrow_ref_mut(cs).write_byte(byte)
    }

    fn pop_txbuf_byte(&self, cs: CriticalSection) -> Option<u8> {
        self.send_buffer.borrow_ref_mut(cs).next()
    }

    fn reset_txbuf(&self, cs: CriticalSection) -> usize {
        let mut sb = self.send_buffer.borrow_ref_mut(cs);
        let bytes_left = sb.bytes_sent();
        sb.reset();
        bytes_left
    }
}
