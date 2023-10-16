use core::mem::MaybeUninit;

pub struct ReceiveBuffer<const BUFSIZE: usize> {
    buf: MaybeUninit<[u8; BUFSIZE]>,
    size: usize,
}

impl<const BUFSIZE: usize> ReceiveBuffer<BUFSIZE> {
    pub const fn new() -> Self {
        Self {
            buf: MaybeUninit::uninit(),
            size: 0,
        }
    }

    pub fn write_byte(&mut self, byte: u8) -> Result<(), ()> {
        if self.size == BUFSIZE {
            Err(())
        } else {
            unsafe { self.buf.assume_init_mut()[self.size] = byte }
            self.size += 1;
            Ok(())
        }
    }

    pub fn get_size(&self) -> usize {
        self.size
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, usize> {
        if buf.len() < self.size {
            Err(self.size)
        } else {
            buf[..self.size].copy_from_slice(unsafe { &self.buf.assume_init_ref()[..self.size] });
            Ok(self.size)
        }
    }

    pub fn reset(&mut self) {
        self.size = 0;
    }
}
