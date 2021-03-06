use std::fmt;
use std::mem;
use std::ops::{DerefMut, Deref};
use utils::Baseable;

const SIZE: usize = 64;

#[derive(Copy)]
pub struct RingBuffer<T> {
    buff: [T; SIZE],
    start: usize,
    count: usize,
}

impl<T: fmt::Debug> fmt::Debug for RingBuffer<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f,
                 "RingBuffer: {:?}, start: {}, count {}",
                 &self.buff as &[T],
                 self.start,
                 self.count)
    }
}

impl<T: Copy + Clone> Clone for RingBuffer<T> {
    fn clone(&self) -> RingBuffer<T> {
        *self
    }
}

#[derive(Debug)]
pub struct BiasedRingBuffer<T> {
    buff: RingBuffer<T>,
    base: T,
}

impl<T: Copy> Clone for BiasedRingBuffer<T> {
    fn clone(&self) -> BiasedRingBuffer<T> {
        BiasedRingBuffer {
            buff: self.buff.clone(),
            base: self.base,
        }
    }
}

impl<T> BiasedRingBuffer<T>
    where T: for<'x> Baseable<&'x T> + Default + Copy
{
    pub fn new() -> Self {
        BiasedRingBuffer {
            buff: RingBuffer::new(),
            base: T::default(),
        }
    }

    pub fn pop_front_biased(&mut self) -> Option<T> {
        self.buff.pop_front().map(|mut e| {
                                      e.sub_base(&self.base);
                                      e
                                  })
    }

    pub fn push_back_biased(&mut self, mut element: T) {
        element.add_base(&self.base);
        self.buff.push_back(element)
    }

    pub fn set_base(&mut self, base: T) {
        self.base = base;
    }
}

impl<T> DerefMut for BiasedRingBuffer<T> {
    fn deref_mut(&mut self) -> &mut RingBuffer<T> {
        &mut self.buff
    }
}

impl<T> Deref for BiasedRingBuffer<T> {
    type Target = RingBuffer<T>;
    fn deref(&self) -> &RingBuffer<T> {
        &self.buff
    }
}

impl<T> AsRef<RingBuffer<T>> for BiasedRingBuffer<T> {
    fn as_ref(&self) -> &RingBuffer<T> {
        &self.buff
    }
}

impl<T> AsMut<RingBuffer<T>> for BiasedRingBuffer<T> {
    fn as_mut(&mut self) -> &mut RingBuffer<T> {
        &mut self.buff
    }
}

impl<T: Copy> RingBuffer<T> {
    pub fn new() -> Self {
        RingBuffer {
            buff: unsafe { mem::uninitialized() },
            start: 0,
            count: 0,
        }
    }

    #[inline]
    pub fn flush(&mut self) {
        self.start = 0;
        self.count = 0;
    }

    pub fn push_back(&mut self, element: T) {
        debug_assert!(self.count < SIZE);
        self.buff[(self.start + self.count) % SIZE] = element;
        self.count += 1;
    }

    pub fn pop_front(&mut self) -> Option<T> {
        if self.count > 0 {
            let element = Some(self.buff[self.start]);
            self.count -= 1;
            self.start += 1;
            self.start %= SIZE;
            element
        } else {
            None
        }
    }

    pub fn peek_front(&self) -> Option<&T> {
        if self.count > 0 {
            Some(&self.buff[self.start])
        } else {
            None
        }
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}


#[cfg(test)]
mod tests {
    use super::{SIZE, RingBuffer};

    #[test]
    fn creating() {
        let buffer = RingBuffer::<u64>::new();
        assert_eq!(buffer.count(), 0);
    }

    #[test]
    fn push_back() {
        let mut buffer = RingBuffer::<u64>::new();
        buffer.push_back(10);
        assert_eq!(buffer.count(), 1);
    }

    #[test]
    fn pop_back() {
        let mut buffer = RingBuffer::<u64>::new();
        buffer.push_back(10);
        assert_eq!(buffer.count(), 1);
        assert_eq!(buffer.pop_front(), Some(10));
        assert_eq!(buffer.count(), 0);
    }

    #[test]
    fn pop_front() {
        let mut buffer = RingBuffer::<u64>::new();
        buffer.push_back(10);
        assert_eq!(buffer.count(), 1);
        assert_eq!(buffer.pop_front(), Some(10));
        assert_eq!(buffer.count(), 0);
    }

    #[test]
    fn extended_front() {
        let mut buffer = RingBuffer::<u64>::new();
        buffer.push_back(5);
        buffer.push_back(10);
        buffer.push_back(15);
        // 5, 10, 15
        assert_eq!(buffer.count(), 3);
        assert_eq!(buffer.pop_front(), Some(5));
        assert_eq!(buffer.count(), 2);
        assert_eq!(buffer.pop_front(), Some(10));
        assert_eq!(buffer.count(), 1);
    }

    #[test]
    fn full() {
        let mut buffer = RingBuffer::new();
        for i in 0..SIZE {
            buffer.push_back(i);
        }
        assert_eq!(buffer.count(), SIZE);
        assert_eq!(buffer.pop_front(), Some(0));
        assert_eq!(buffer.pop_front(), Some(1));
        assert_eq!(buffer.count(), SIZE - 2);
    }

    #[test]
    fn empty() {
        let mut buffer = RingBuffer::<usize>::new();
        assert_eq!(buffer.pop_front(), None);
    }

    #[test]
    fn flush() {
        let mut buffer = RingBuffer::new();
        buffer.push_back(10);
        buffer.push_back(9);
        assert_eq!(buffer.count(), 2);
        assert_eq!(buffer.pop_front(), Some(10));
        assert_eq!(buffer.count(), 1);
        buffer.flush();
        assert_eq!(buffer.pop_front(), None);
        assert!(buffer.is_empty());
    }
}
