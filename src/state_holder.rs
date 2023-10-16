use core::cell::{Ref, RefCell};

use atomic::{Atomic, Ordering};
use critical_section::{CriticalSection, Mutex};
use heapless::Deque;

use crate::State;

pub struct StateHolder<const HISTORY_SIZE: usize> {
    history: Mutex<RefCell<Deque<State, HISTORY_SIZE>>>,
    state: Atomic<State>,
}

impl<const HISTORY_SIZE: usize> StateHolder<HISTORY_SIZE> {
    pub const fn new() -> Self {
        Self {
            history: Mutex::new(RefCell::new(Deque::new())),
            state: Atomic::new(State::Idle),
        }
    }

    pub fn set_state(&self, state: State) {
        self.add_state_in_history(state);
        self.state.store(state, Ordering::SeqCst);
    }

    pub fn get_state(&self) -> State {
        self.state.load(Ordering::SeqCst)
    }

    #[allow(dead_code)]
    pub fn get_history<'cs>(
        &'cs self,
        cs: CriticalSection<'cs>,
    ) -> Ref<'cs, Deque<State, HISTORY_SIZE>> {
        self.history.borrow_ref(cs)
    }

    fn add_state_in_history(&self, state: State) {
        critical_section::with(|cs| {
            let mut h = self.history.borrow_ref_mut(cs);
            if h.is_full() {
                h.pop_front();
            }
            h.push_back(state).unwrap();
        });
    }
}
