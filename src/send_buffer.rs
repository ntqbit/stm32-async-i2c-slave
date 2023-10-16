use core::{cmp, mem::MaybeUninit};

pub struct SendBuffer<const BUFSIZE: usize> {
    buf: MaybeUninit<[u8; BUFSIZE]>,
    pos: usize,
    end: usize,
}

impl<const BUFSIZE: usize> SendBuffer<BUFSIZE> {
    pub const fn new() -> Self {
        Self {
            buf: MaybeUninit::uninit(),
            pos: 0,
            end: 0,
        }
    }

    pub fn write<'a>(&mut self, buf: &'a [u8]) -> &'a [u8] {
        assert!(
            buf.len() <= BUFSIZE,
            "Trying to write too much data into the send buffer"
        );

        if !self.is_empty() {
            panic!("Send buffer must be reset before writing.");
        }

        let take_idx = cmp::min(buf.len(), BUFSIZE);
        unsafe { self.buf.assume_init_mut()[..take_idx].copy_from_slice(buf) };

        self.pos = 0;
        self.end = take_idx;

        &buf[take_idx..]
    }

    pub fn reset(&mut self) {
        self.pos = 0;
        self.end = 0;
    }

    pub fn bytes_sent(&self) -> usize {
        self.pos
    }

    pub fn is_empty(&self) -> bool {
        self.end == self.pos
    }
}

impl<const BUFSIZE: usize> Iterator for SendBuffer<BUFSIZE> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            None
        } else {
            self.pos += 1;
            Some(unsafe { self.buf.assume_init_ref()[self.pos - 1] })
        }
    }
}
