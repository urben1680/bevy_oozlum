use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque},
    error::Error,
    fmt::Display,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

// todo: mention limitations in docs, like missing frames
#[derive(Clone, Default, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrameTransitionLog {
    // 0b00xxxxxx = 1 byte, 0b01xxxxxx = 1+n+1 bytes where n are 0b1xxxxxxx
    offset_bytes: VecDeque<u8>,
    oldest_run: u64,
    last_run: u64,
    index: usize,
    past_len: usize
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::FrameTransitionLog;

    impl WithCapacity for FrameTransitionLog {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<u8>>, u64, u64, usize, usize);
        type De = (WithCapacityWrapper<VecDeque<u8>>, u64, u64, usize, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.offset_bytes), self.oldest_run, self.last_run, self.index, self.past_len)
        }
        fn from_with_capacity((WithCapacityWrapper(offset_bytes), oldest_run, last_run, index, past_len): Self::De) -> Self {
            Self { offset_bytes, oldest_run, last_run, index, past_len }
        }
    }

    impl LoglessWithCapacity for FrameTransitionLog {
        type Se<'se> = usize;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.bytes_capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl Debug for FrameTransitionLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameTransitionLog")
            .field("offset_bytes", &self.offset_bytes)
            .field("offsets()", &VarLenOffsetsIter::<true> {
                offset_bytes: &self.offset_bytes,
                index: 0
            })
            .field("oldest_run", &self.oldest_run)
            .field("last_run", &self.last_run)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .finish()
    }
}

#[derive(Clone)]
struct VarLenOffsetsIter<'a, const FORWARD: bool> {
    offset_bytes: &'a VecDeque<u8>,
    index: usize,
}

impl Debug for VarLenOffsetsIter<'_, true> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

// does not impl DoubleEndedIterator because it requires to return None when they "meet in the middle"
// and that is unneeded but would require more runtime checks.
impl<const FORWARD: bool> Iterator for VarLenOffsetsIter<'_, FORWARD> {
    type Item = u64;
    fn next(&mut self) -> Option<Self::Item> {
        /*
        todo: zero offset optimization
        The first read byte is expected to have 0 as the most significant bit so it can be distinguished
        from the bytes that are wrapped by 0b01_xxxxxx bytes.
        However, that does not matter for the 0b00_xxxxxx byte, it could very well be a 0b1_xxxxxx encoded.
        But instead, a 0b1_xxxxxxx byte could have an alternative meaning: the xxxxxxx could encode the
        amount of zero offsets at this index. 0b00_000000 could still be interpreted as "one zero offset",
        but anything more could be encoded in 0b1xxxxxxx with a minimum of "two zero offsets" up to 129 zeroes.
        This could further optimize this log for cases where something is updated more than once per frame.

        Doing this however requires extra fields in the iterator and the log that counts the current position
        in the amount of zeroes, comparable to the sparse logs.
            */
        if FORWARD {
            let byte = *self.offset_bytes.get(self.index)?;
            let data = byte & 0b00_111111;
            self.index += 1;
            if byte == data {
                return Some(data as u64);
            }
            let mut frame = data as u64;
            let mut shift = 6;
            loop {
                let byte = self.offset_bytes[self.index];
                self.index += 1;
                if byte.leading_zeros() != 0 {
                    return Some(frame | (((byte & 0b00_111111) as u64) << shift));
                }
                frame |= ((byte & 0b0_1111111) as u64) << shift;
                shift += 7;
            }
        } else {
            if self.index == 0 {
                return None;
            }
            self.index -= 1;
            let byte = self.offset_bytes[self.index];
            let data = byte & 0b00_111111;
            if byte == data {
                return Some(data as u64);
            }
            let mut frame = data as u64;
            loop {
                self.index -= 1;
                let byte = self.offset_bytes[self.index];
                if byte.leading_zeros() != 0 {
                    return Some((frame << 6) | (byte & 0b00_111111) as u64);
                }
                frame = (frame << 7) | (byte & 0b0_1111111) as u64;
            }
        }

    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.offset_bytes.is_empty() {
            return (0, Some(0));
        }
        // at most 10 bytes are used to store a u64
        (self.offset_bytes.len().div_ceil(10), Some(self.offset_bytes.len()))
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissedFrame(pub u64);

impl Display for MissedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the expected frame {} was missed", self.0)
    }
}

impl Error for MissedFrame {}

impl FrameTransitionLog {
    pub const fn new() -> Self {
        Self {
            offset_bytes: VecDeque::new(),
            oldest_run: 0,
            last_run: 0,
            index: 0,
            past_len: 0
        }
    }
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            oldest_run: 0,
            last_run: 0,
            index: 0,
            past_len: 0
        }
    }
    pub fn bytes_len(&self) -> usize {
        self.offset_bytes.len()
    }
    pub fn bytes_capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }
    pub fn bytes_is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }
    pub fn truncate_future(&mut self) {
        self.offset_bytes.truncate(self.index);
    }
    pub fn clear(&mut self) {
        self.offset_bytes.clear();
        self.index = 0;
    }
    pub fn push_and_get_past_len(&mut self, meta: &RevMeta) -> usize {
        // special case empty log
        if self.offset_bytes.is_empty() {
            self.oldest_run = meta.past_end();
            self.last_run = meta.past_end();
        } else {
            // truncate future
            self.truncate_future();

            // truncate past
            if self.oldest_run < meta.past_end() {
                let mut iter = VarLenOffsetsIter::<true> {
                    offset_bytes: &self.offset_bytes,
                    index: 0
                };

                for offset in iter.by_ref() {
                    self.oldest_run += offset;
                    self.past_len -= 1;
                    if self.oldest_run >= meta.past_end() {
                        break;
                    }
                }

                let mut drain_to = iter.index;
                loop {
                    if Some(0) != iter.next() {
                        break;
                    }
                    self.past_len -= 1;
                    drain_to = iter.index;
                }

                self.offset_bytes.drain(..drain_to); // https://github.com/rust-lang/rust/issues/140667
                self.index = self.offset_bytes.len();
            }
        }

        // push present offset
        let mut offset = meta.now() - self.last_run;
        self.last_run = meta.now();
        self.past_len += 1;
        self.index += 1;

        if offset <= 0b00_111111 {
            self.offset_bytes.push_back(offset as u8);
            return self.past_len;                
        }

        self.offset_bytes.push_back((offset & 0b00_111111) as u8 | 0b01_000000);
        offset >>= 6;

        loop {
            self.index += 1;
            if offset <= 0b00_111111 {
                self.offset_bytes.push_back(offset as u8  | 0b01_000000);
                return self.past_len;
            }
            self.offset_bytes.push_back((offset & 0b0_1111111) as u8 | 0b1_0000000);
            offset >>= 7;
        }
    }
    pub fn try_backward_log(&mut self, meta: &RevMeta) -> Result<bool, MissedFrame> {
        let mut iter = VarLenOffsetsIter::<false> {
            offset_bytes: &self.offset_bytes,
            index: self.index
        };
        let Some(previous_offset) = iter.next() else {
            return Ok(false);
        };
        match self.last_run.cmp(&(meta.now() + 1)) {
            Ordering::Less => Ok(false),
            Ordering::Equal => {
                self.last_run -= previous_offset;
                self.index = iter.index;
                self.past_len -= 1;
                Ok(true)
            }
            Ordering::Greater => Err(MissedFrame(self.last_run)),
        }
    }
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        self.try_backward_log(meta).unwrap()
    }
    pub fn try_forward_log(&mut self, meta: &RevMeta) -> Result<bool, MissedFrame> {
        let mut iter = VarLenOffsetsIter::<true> {
            offset_bytes: &self.offset_bytes,
            index: self.index
        };
        let Some(next_offset) = iter.next() else {
            return Ok(false);
        };
        let frame = self.last_run + next_offset;
        match frame.cmp(&meta.now()) {
            Ordering::Greater => Ok(false),
            Ordering::Equal => {
                self.index = iter.index;
                self.last_run = frame;
                self.past_len += 1;
                Ok(true)
            }
            Ordering::Less => Err(MissedFrame(frame)),
        }
    }
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        self.try_forward_log(meta).unwrap()
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

    use super::*;

    #[test]
    fn offset_iter_works() {
        let offsets = [
            0b___________________________________________________________________000000,
            0b____________________________________________________________000010_000001,
            0b____________________________________________________000101_0000100_000011,
            0b____________________________________________001001_0001000_0000111_000110,
            0b____________________________________001110_0001101_0001100_0001011_001010,
            0b____________________________010100_0010011_0010010_0010001_0010000_001111,
            0b____________________011011_0011010_0011001_0011000_0010111_0010110_010101,
            0b____________100011_0100010_0100001_0100000_0011111_0011110_0011101_011100,
            0b____101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100,
            0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101
        ];

        let deque = [
            // 0b000000
            0b00_000000,

            // 0b000010_000001
            0b01_000001,
            0b01_000010,

            // 0b000101_0000100_000011
            0b01_000011,
            0b1_0000100,
            0b01_000101,

            // 0b001001_0001000_0000111_000110
            0b01_000110,
            0b1_0000111,
            0b1_0001000,
            0b01_001001,

            // 0b001110_0001101_0001100_0001011_001010
            0b01_001010,
            0b1_0001011,
            0b1_0001100,
            0b1_0001101,
            0b01_001110,

            // 0b010100_0010011_0010010_0010001_0010000_001111
            0b01_001111,
            0b1_0010000,
            0b1_0010001,
            0b1_0010010,
            0b1_0010011,
            0b01_010100,

            // 0b011011_0011010_0011001_0011000_0010111_0010110_010101
            0b01_010101,
            0b1_0010110,
            0b1_0010111,
            0b1_0011000,
            0b1_0011001,
            0b1_0011010,
            0b01_011011,

            // 0b100011_0100010_0100001_0100000_0011111_0011110_0011101_011100
            0b01_011100,
            0b1_0011101,
            0b1_0011110,
            0b1_0011111,
            0b1_0100000,
            0b1_0100001,
            0b1_0100010,
            0b01_100011,

            // 0b101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100
            0b01_100100,
            0b1_0100101,
            0b1_0100110,
            0b1_0100111,
            0b1_0101000,
            0b1_0101001,
            0b1_0101010,
            0b1_0101011,
            0b01_101100,

            // 0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101
            0b01_101101,
            0b1_0101110,
            0b1_0101111,
            0b1_0110000,
            0b1_0110001,
            0b1_0110010,
            0b1_0110011,
            0b1_0110100,
            0b1_0110101,
            0b01_0000_10 // only least significant two bits are available, more would overflow u64
        ].into();

        let mut iter = VarLenOffsetsIter::<true> {
            offset_bytes: &deque,
            index: 0
        };
        let iter = iter.by_ref();

        let actual = iter.next().unwrap();
        let expected = offsets[0];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 1);

        let actual = iter.next().unwrap();
        let expected = offsets[1];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 3);

        let actual = iter.next().unwrap();
        let expected = offsets[2];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 6);

        let actual = iter.next().unwrap();
        let expected = offsets[3];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 10);

        let actual = iter.next().unwrap();
        let expected = offsets[4];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 15);

        let actual = iter.next().unwrap();
        let expected = offsets[5];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 21);

        let actual = iter.next().unwrap();
        let expected = offsets[6];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 28);

        let actual = iter.next().unwrap();
        let expected = offsets[7];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 36);

        let actual = iter.next().unwrap();
        let expected = offsets[8];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 45);

        let actual = iter.next().unwrap();
        let expected = offsets[9];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, deque.len());

        assert_eq!(iter.next(), None);
        assert_eq!(iter.index, deque.len());

        let mut iter = VarLenOffsetsIter::<false> {
            offset_bytes: &deque,
            index: deque.len()
        };
        let iter = iter.by_ref();
        
        let actual = iter.next().unwrap();
        let expected = offsets[9];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 45);
        
        let actual = iter.next().unwrap();
        let expected = offsets[8];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 36);
        
        let actual = iter.next().unwrap();
        let expected = offsets[7];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 28);
        
        let actual = iter.next().unwrap();
        let expected = offsets[6];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 21);
        
        let actual = iter.next().unwrap();
        let expected = offsets[5];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 15);
        
        let actual = iter.next().unwrap();
        let expected = offsets[4];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 10);
        
        let actual = iter.next().unwrap();
        let expected = offsets[3];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 6);
        
        let actual = iter.next().unwrap();
        let expected = offsets[2];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 3);
        
        let actual = iter.next().unwrap();
        let expected = offsets[1];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 1);
        
        let actual = iter.next().unwrap();
        let expected = offsets[0];
        assert_eq!(actual, expected, "{actual:b}, {expected:b}");
        assert_eq!(iter.index, 0);
        
        assert_eq!(iter.next(), None);
        assert_eq!(iter.index, 0);
    }

    struct Log {
        log: FrameTransitionLog,
        meta: RevMeta,
    }

    impl Log {
        fn new(max_world_states: u64, now: u64) -> Self {
            let log = FrameTransitionLog::new();
            let meta = RevMeta::new(NonZeroU64::new(max_world_states), now, false);
            Self { log, meta }
        }
        fn forward(&mut self, updates_with_expected_past_len: Vec<usize>) {
            self.meta.queue_not_log_forward();
            self.meta
                .update(|meta| {
                    let before = self.log.clone();
                    let len = updates_with_expected_past_len.len();
                    let updates_with_actual_past_len: Vec<usize> = (0..len)
                        .map(|_| self.log.push_and_get_past_len(meta))
                        .collect();
                    assert_eq!(
                        updates_with_actual_past_len, updates_with_expected_past_len,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    )
                })
                .expect("should update");
        }
        fn forward_log(&mut self, expected_forward_log_updates: usize) {
            let previous = self.meta.now() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_forward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.forward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.forward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
        fn backward_log(&mut self, expected_backward_log_updates: usize) {
            let previous = self.meta.now() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_backward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.backward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.backward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut log = Log::new(4, 0);
        log.forward(vec![1]); // frame #1
        log.forward(vec![2, 3]); // frame #2
        log.forward(vec![4]);
        log.forward(vec![]);
        // shortened log of runs from frame #1 and #2
        log.forward(vec![2, 3]); // !! this changed from vec![4, 5]

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward_log(1);
        log.forward_log(0);
        log.forward_log(2);

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward(vec![1]);
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut log = Log::new(4, 0);

        log.forward(vec![1]); // frame #1
        assert_eq!(log.meta.past_len(), 1);

        log.forward(vec![2]); // frame #2
        assert_eq!(log.meta.past_len(), 2);
        
        log.forward(vec![3]);
        assert_eq!(log.meta.past_len(), 3);
        
        log.forward(vec![3]);
        assert_eq!(log.meta.past_len(), 3);
    }
}
