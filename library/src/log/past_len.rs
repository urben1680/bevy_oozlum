use core::fmt::Debug;
use std::{
    cmp::Ordering as CmpOrdering, collections::{vec_deque::Iter, TryReserveError, VecDeque}, error::Error, fmt::Display, num::NonZeroUsize, ops::ControlFlow, panic::Location, sync::{atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering as AtomicOrdering}, Arc}, u32
};

use bevy::ecs::{change_detection::MaybeLocation, resource::Resource};

use crate::{log::OutOfLog, meta::{RevDirection, RevMeta}};

const MAX_ZEROES_PER_BYTE: u8 = 65;
const MAX_ZEROES_AS_BYTE: u8 = 0b10_111111;
const ZEROES_MASK: u8 = 0b00_111111;
const ZEROES_OR: u8 = 0b10_000000;
const MAX_SINGLE_BYTE_OFFSET: u8 = 0b0_1111111;
const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const WRAPPING_OFFSET_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const MAX_WRAPPING_OFFSET: u8 = 0b00_111111;

/// A log that keeps track when it was updated and provides an alternative value to
/// [`RevMeta::past_len`] for when these updates do not happen exactly once per [`RevUpdate`].
///
/// It is generally adviced to not update this from multiple systems as debugging becomes easier
/// if instead each system has its own `PastLenLog`.
///
/// # Examples
///
/// This log is usually used next to other [logs] that need a `max_past_len` value to determine
/// when they can shorten their log entries once these go out of log. These logs do _not_ need
/// a `PastLenLog` if they get updated exactly once every frame:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy::ecs::error::Result;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseStateLog;
/// # #[derive(Default)]
/// # struct MyType;
/// # impl MyType { fn new() -> Self { Self }}
/// # let mut app = App::new();
/// app.rev_add_systems(RevUpdate, my_system); // runs once per reversible frame
///
/// // needs no PastLenLog
/// fn my_system(meta: Res<RevMeta>, mut value_log: Local<DenseStateLog<MyType>>) -> Result {
///     match meta.running_direction() {
///         RevDirection::Forward { log } => {
///             if log {
///                 value_log.forward_log()?;
///             } else {
///                 value_log.push_and_pop_past(meta.past_len() as usize, MyType::new());
///             }
///             // deref value_log to use value
///         }
///         RevDirection::BackwardLog => {
///             // deref value_log to use value
///             value_log.backward_log()?;
///         }
///     }
///     Ok(())
/// }
/// ```
///
/// However, when you intend to update your log less often than that and `MyType` is large enough
/// that you are concerned with the size if you let it allow to grow up to [`RevMeta::past_len`],
/// you might want to use a `Sparse*` log instead (see the [logs] module) or add an `PastLenLog`
/// next to your log.
///
/// If you want to update your log more often than once per frame or generally in anarbitrary
/// manner, you might want to use a `PastLenLog` next to it:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy::ecs::error::Result;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # use bevy_oozlum::log::PastLenLog;
/// # #[derive(BufferedEvent, Clone)]
/// # struct MyEvent;
/// fn my_system(
///     meta: Res<RevMeta>,
///     mut events: EventReader<MyEvent>,
///     mut past_len_log: Local<PastLenLog>,
///     mut events_log: Local<DenseTransitionLog<MyEvent>>,
/// ) -> Result {
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             // always truncate the future of the logs in case there are no events
///             past_len_log.truncate_future(&meta)?;
///             events_log.drain_future();
///
///             // there may be 0, 1 or more events per system run
///             for my_event in events.read() {
///                 // use event
///
///                 // past_len contains just the right value that events_log is shortened the
///                 // minimum amount of events to go back and forth to any point of the global
///                 // log and not a single event more
///                 let past_len = past_len_log.update_and_get_past_len(&meta)?;
///                 events_log.push_and_drain_past(past_len, my_event.clone());
///             }
///         }
///         RevDirection::FORWARD_LOG => {
///             while past_len_log.forward_log(&meta)? {
///                 let my_event = events_log.forward_log()?;
///                 // use event
///             }
///         }
///         RevDirection::BackwardLog => {
///             while past_len_log.backward_log(&meta)? {
///                 let my_event = events_log.backward_log()?;
///                 // use event
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
///
/// [logs]: crate::log
/// [`RevUpdate`]: crate::schedule::RevUpdate
#[derive(Clone, Default)]
pub struct PastLenLog {
    /// Contains the offsets between the [frames](RevMeta::now) this log was updated at.
    ///
    /// The offsets are encoded in a special way to minimize the used memory as typically the
    /// use-cases have the task to keep the memory overhead low.
    ///
    /// - Offset from `0` to `127` are encoded in a single byte as `x` bits in the pattern of
    ///   `0b0_xxxxxxx`.
    /// - Up to `65` sequential offsets of `0` are encoded in a single byte as `x` bits in the
    ///   pattern of `0b10_xxxxxx`. The numeric value of the `x` is actually read plus 2. This is
    ///   because:
    ///   - There is no concept of "zero times an offset of `0`" so `0b10_000000` makes no sense to
    ///     be interpreted as "zero times".
    ///   - The value of "one time an offset of `0`" is already encoded in `0b0_0000000`.
    /// - Offsets larger than `127` are encoded in multiple bytes and are split in chunks of `x`
    ///   bits:
    ///   - The first and last byte of this sequence use the pattern `0b11_xxxxxx`.
    ///   - If more bits are needed, in between are bytes that use the pattern `0b0_xxxxxxx`.
    ///   - This uses up to ten bytes in total for `u64::MAX`
    /// - These bytes or sequences of bytes can be read in reverse as well, which is needed for
    ///   reading the previous offset in [`Self::backward_log`].
    /// - The [`OffsetIter`] iterator is used to read the offsets. See [`IterItem`].
    offset_bytes: VecDeque<u8>,

    /// A frame this log matched at (or 0) that is either at [`RevMeta::past_end`] or as closely
    /// before it as possible to determine how much other logs that run along this can be reduced
    /// to.
    ///
    /// This frame must not be a more recent frame because then [`Self::backward_log`] will be
    /// unable to match that frame as [`Self::index`] cannot be reduced further. Otherwise, the
    /// [`OutOfLog`] error is returned which is usually not encountered with this log.
    out_of_or_past_end_log: u64,

    /// The chronological last time this log got updated
    last_run: u64,

    /// The current index into [`Self::offset_bytes`]. Always points to the first byte of an
    /// offset sequence or, when at the end of the log, is equal to the `len` of `offset_bytes`.
    index: usize,

    /// The length of the log which is what this log is keeping track of.
    past_len: usize,

    direction_changes_seen: usize,

    /// The current amount of sequential offsets of `0`.
    zeroes: u8,

    /// The amount of sequential offsets of `0` at the future end of the log.
    zeroes_max: u8,
}

#[cfg(feature = "serialize")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serialize::WithCapacity;

    use super::PastLenLog;

    impl Serialize for PastLenLog {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer {
            ().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for PastLenLog {
        fn deserialize<D>(_: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de> {
            Ok(Self::new())
        }
    }

    impl WithCapacity for PastLenLog {
        type Se<'se> = usize;
        type De = usize;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            self.bytes_capacity()
        }
        fn from_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl Debug for PastLenLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastLenLog")
            .field("offset_bytes", &self.offset_bytes)
            .field("offsets (decoded)", &OffsetIter(self.offset_bytes.iter()))
            .field("out_of_or_past_end_log", &self.out_of_or_past_end_log)
            .field("last_run", &self.last_run)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .field("zeroes", &self.zeroes)
            .field("zeroes_max", &self.zeroes_max)
            .finish()
    }
}

/// Iterator to read [`PastLenLog::offset_bytes`] and decode them to [`IterItem`].
#[derive(Clone)]
struct OffsetIter<'a>(Iter<'a, u8>);

/// Reads a byte or sequence of bytes from [`PastLenLog::offset_bytes`]. Contains a single offset
/// or, if the offset is `0`, a sequence of such offsets.
#[derive(Debug, PartialEq, Clone)]
struct IterItem {
    /// Amount of frames between two updates of [`PastLenLog`].
    offset: u64,

    /// The amount of bytes this offset is made of to update [`PastLenLog::index`] correctly.
    /// If [Self::offset] == `0`, this is the amount of `0` offsets in this byte instead.
    len: NonZeroUsize,
}

impl Debug for OffsetIter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.clone().flat_map(|IterItem { offset, len }| {
                let count = if offset == 0 { len.get() } else { 1 };
                core::iter::repeat_n(offset, count)
            }))
            .finish()
    }
}

/// Decode first byte that is read by [`OffsetIter`], may be the last byte in the sequence if the
/// iterator goes backwards. Returns [`ControlFlow::Break`] if the offset consists of only one byte.
/// Otherwise returns [`ControlFlow::Continue`] with the first bits of the offset that the iterator
/// has to complete.
fn check_first_byte(byte: u8) -> ControlFlow<IterItem, u64> {
    match byte.leading_ones() {
        // 0b0_xxxxxxx => a single-byte offset
        0 => ControlFlow::Break(IterItem {
            offset: byte as u64,
            len: NonZeroUsize::MIN,
        }),
        // 0b10_xxxxxx => sequence of offsets of 0 in a single byte
        1 => {
            // up to 65 zeroes, composed of 0b00_111111 = 63 ...
            // ... + 1 because it is always at least one zero
            // ... + 1 because above match arm already decodes 0b0_0000000 to len 1
            let zeroes = (byte & ZEROES_MASK) + 2;
            ControlFlow::Break(IterItem {
                offset: 0,
                // SAFETY: `zeroes` cannot be zero, +2 could not overflow with MSBs masked away
                len: unsafe { NonZeroUsize::new_unchecked(zeroes as usize) },
            })
        }
        // 0b11_xxxxxx => wrapping byte of a multi-byte offset
        _ => ControlFlow::Continue((byte & WRAPPING_OFFSET_MASK) as u64),
    }
}

impl Iterator for OffsetIter<'_> {
    type Item = IterItem;
    fn next(&mut self) -> Option<Self::Item> {
        // check first byte
        let byte = *self.0.next()?;
        let mut offset = match check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(offset) => offset,
        };

        // this is a multi-byte offset

        let mut len = 1;

        // wrapping bytes contain 6 usable bits for the offset
        let mut shift = 6;

        loop {
            let byte = *self.0.next().unwrap(); // encoding expects more bytes to follow

            len += 1;

            if byte.leading_zeros() == 0 {
                // this is a wrapping byte

                // the added bits are more significant
                offset |= ((byte & WRAPPING_OFFSET_MASK) as u64) << shift;
                return Some(IterItem {
                    offset,
                    // SAFETY: len started with 1, could be at most 10, never overflows
                    len: unsafe { NonZeroUsize::new_unchecked(len) },
                });
            }

            // this is a wrapped byte

            // the added bits are more significant, has no marker bits that need to be masked away
            offset |= (byte as u64) << shift;

            // wrapped bytes contain 7 usable bits for the offset
            shift += 7;
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.len();

        // at most 10 bytes are used to store a u64
        let min = len.div_ceil(10);

        // up to 65 zeroes can be stored in a byte
        let max = len.saturating_mul(MAX_ZEROES_PER_BYTE as usize);

        (min, Some(max))
    }
}

impl DoubleEndedIterator for OffsetIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        // check first byte
        let byte = *self.0.next_back()?;
        let mut offset = match check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(offset) => offset,
        };

        // this is a multi-byte offset

        let mut len = 1;

        loop {
            let byte = *self.0.next_back().unwrap(); // encoding expects more bytes to follow

            len += 1;

            if byte.leading_zeros() == 0 {
                // this is a wrapping byte

                // the added bits are less significant, wrapping bytes contain 6 usable bits for the
                // offset
                offset = (offset << 6) | (byte & WRAPPING_OFFSET_MASK) as u64;

                return Some(IterItem {
                    offset,
                    // SAFETY: len started with 1, could be at most 10, never overflows
                    len: unsafe { NonZeroUsize::new_unchecked(len) },
                });
            }

            // this is a wrapped byte

            // the added bits are less significant, has no marker bits that need to be masked away,
            // wrapped bytes contain 7 usable bits for the offset
            offset = (offset << 7) | byte as u64;
        }
    }
}

/// [`PastLenLog`] was updated via [`forward_log`]/[`backward_log`] but expected [`RevMeta::now`] to
/// return a frame that was missed; it is too high for `forward_log` or too low for `backward_log`.
///
/// Make sure use these methods at every frame the log could have been updated during
/// [`RevDirection::NOT_LOG`]. If it can be updated multiple times per frame, call these methods in
/// a `while` loop.
///
/// [`forward_log`]: PastLenLog::forward_log
/// [`backward_log`]: PastLenLog::backward_log
/// [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissedUpdate(pub u64);

impl Display for MissedUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the expected frame {} was missed", self.0)
    }
}

impl Error for MissedUpdate {}

/// [`PastLenLog`] was updated via [`backward_log`] but returned this error. In most cases the error
/// is the [`MissedUpdate`] variant, see its docs.
///
/// If this is the [`OutOfLog`] variant, this indicates [`RevMeta::past_end`] has been lowered
/// somehow, which is not possible by usual means, or `PastLenLog` has been deserialized from an
/// invalid state.
///
/// [`backward_log`]: PastLenLog::backward_log
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum PastLenBackwardError {
    MissedUpdate(MissedUpdate),
    OutOfLog(OutOfLog),
}

impl Display for PastLenBackwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissedUpdate(err) => Display::fmt(err, f),
            Self::OutOfLog(err) => Display::fmt(err, f),
        }
    }
}

impl Error for PastLenBackwardError {}

impl From<MissedUpdate> for PastLenBackwardError {
    fn from(value: MissedUpdate) -> Self {
        Self::MissedUpdate(value)
    }
}

impl From<OutOfLog> for PastLenBackwardError {
    fn from(value: OutOfLog) -> Self {
        Self::OutOfLog(value)
    }
}

/// [`PastLenLog`] was updated via [`update_and_get_past_len`] or [`truncate_future`] but returned
/// this error.
///
/// This indicates one or more [`forward_log`] or [`backward_log`] calls were missed. Make sure to
/// call these in every frame where the log is updated during [RevDirection::NOT_LOG].
///
/// [`update_and_get_past_len`]: PastLenLog::update_and_get_past_len
/// [`truncate_future`]: PastLenLog::truncate_future
/// [`forward_log`]: PastLenLog::forward_log
/// [`backward_log`]: PastLenLog::backward_log
/// [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum PastLenNotLogError {
    MissedUpdateForwardLog(MissedUpdate),
    MissedUpdateBackwardLog(MissedUpdate),
}

impl Display for PastLenNotLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissedUpdateForwardLog(err) => write!(f, "{err} during some forward log update"),
            Self::MissedUpdateBackwardLog(err) => {
                write!(f, "{err} during some backward log update")
            }
        }
    }
}

impl Error for PastLenNotLogError {}

macro_rules! bytes_len_disclaimer {
    () => {
        "\nNote that the number of bytes have no relation to the length of the log."
    };
}

impl PastLenLog {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            offset_bytes: VecDeque::new(),
            out_of_or_past_end_log: 0, // the minimum frame RevUpdate can go forward at is 1
            last_run: 0,
            index: 0,
            past_len: 0,
            direction_changes_seen: 0,
            zeroes: 0,
            zeroes_max: 0,
        }
    }

    /// Creates an empty log with space for at least `bytes_capacity` bytes.
    ///
    /// See [`VecDeque::with_capacity`].
    #[doc = bytes_len_disclaimer!()]
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            ..Self::new()
        }
    }

    /// Returns the number of bytes in the log.
    ///
    /// See [`VecDeque::len`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_len(&self) -> usize {
        self.offset_bytes.len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }

    /// Remove the logged runs that are in the future.
    ///
    /// This returns an error if one or more [`Self::backward_log`]/[`Self::forward_log`] calls
    /// were missed. See [`PastLenNotLogError`].
    ///
    /// This **must** be called during [`RevDirection::NOT_LOG`] in frames where
    /// [`Self::update_and_get_past_len`] is not used but later
    /// [`backward_log`]/[`forward_log`] are tasked to determine if the log has been
    /// updated. Otherwise errors may not be possible to be detected.
    ///
    /// See the [type docs] for an example.
    ///
    /// [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
    /// [type docs]: PastLenLog
    pub fn truncate_future(&mut self, meta: &RevMeta, direction_changes: &DirectionChanges) -> Result<(), PastLenNotLogError> {
        if self.last_run > meta.now() {
            return Err(PastLenNotLogError::MissedUpdateBackwardLog(MissedUpdate(
                self.last_run,
            )));
        }

        /*

        F/B = non-log-forward und backward von PastLenLog
        f = non-log-forward nur von RevMeta but maybe out of global log
        * as f but witnessed by PastLenLog at its own F

        false-positive 1:
        --*
        --------F
        ----B
        ------f
        ----------F

        false-positive 2:
        --*
        --------F
        ----B
        ------f
        ----------f
        ------------F

        actual error 1:
        --*
        ------F
        ----B
        --------f
        ----------F

        actual error 2:
        --*
        --------F
        ----B
        ----------f
        ------f
        ------------F


        der nächste frame im log für PastLen könnte global out-of-log sein, ein gesuchtes
        log-beenden kann trotzdem einen fehler hier als false-positive erkennbar machen

        RevMeta muss dazu ein log nutzen da das letzte continue nicht notwendigerweise das
        ist was den false-positive hier erkennt.

        offenbar darf RevMeta nicht aufräumen da es immer ein altes PastLen geben kann das
        diese information braucht.

        man könnte etwas sparen indem man neue continue einträge sortiert einfügt und alte
        zukünftige entfernt. das macht es aber schwerer für PastLenLog die info zu finden
        weil der index ungültig wird. Ein sortierter vec ist zwar mit binärsuche nutzbar,
        das hier ist aber eine Funktion die pro frame pro log aufgerufen wird, zu teuer.

        RevMeta könnte ein Vec<(u64, AtomicU32)> nutzen und RevMeta könnte sich merken mit
        welchen index min_continuation angefragt wird und für diesen index einen counter
        reduzieren um den zurückgegebenen zu erhöhen. Die RevMeta Funktion muss dann den
        Fehler evaluieren um nur bei Ok die Zähler anzupassen. Dann, wenn RevMeta updated,
        werden alle trailing entries mit counter 0 entfernt.
        Diese Funktionalität wird besser in einem struct in diesem scope umgesetzt.
        Wie mit drop umgehen? Self::clear mit RevMeta parameter und Drop mit assert auf
        meta_continuations == 0. Ggf drop_command(self) -> impl Command anbieten.
        Alternativ sind es Arc<AtomicU32> im RevMeta log und PastLenLog hält einen.
        Dann kann man aber direkt über die Counter vom Arc gehen und () wrappen

        ideal wäre auch verpasste updates zu erkennen die rein in logphasen passieren,
        zum Beispiel wenn das log zu forward->backward jeweils geupdated worden werden sollte
        es aber nur beim darauffolgenden update aktualisiert wird. Das könnte aber nur
        RevMeta erkennen.
        
        RevMeta könnte ein StatesLog enthalten mit AtomicU8 das zu einem enum übersetzt werden kann
        LogLenLog müsste selbst ein TransitionLog haben das sich speichert unter welchem index
        das AtomicU8 zu finden ist..
        Gibt es einen Ansatz in dem alleine RevMeta prüft ob die logs gelaufen sind? in der
        richtigen anzahl?
        die logs könnten ein Arc<AtomicU32> bei NOT_LOG via Parallel an RevMeta geben
        RevMeta enthält dann ein DenseStatesLog<Arc<AtomicU32>>
        Die logs kümmern sich dann nur darum ihre atomics zu updaten
        RevMeta prüft ob die anzahlen die erwartete Höhe haben
        womöglich lassen sich loglenlogs dann anders umsetzen, keine offsets, nur prüfen ob das
        eigene arc im aktuellen meta state vorkommen

        Neue Idee: wie continue log alle richtungswechsel loggen. Dann kann PastLenLog über den
        letzten index an prüfen ob es verpasste updates gab bis es auf ein non-log eintrag stößt

        Beispiele:
        Not log, Forward log, Backward log
        klein = nur meta, groß = meta und log

        nnNnnNnnn
        -bBbbBbbb <- B should not cause an error because there is a n at an earlier frame below
        -fFff <----- F was last
        ---bb
        ---f
        ----nnnNn <- N is new

        benutzt trotzdem arcs zum reduzieren



        PastLenLog:
        - index: usize
        - reservation: Arc<()>

        RevMeta:
        - Vec<(u64, Arc<()>)>


         */

        if let Some(frame) = meta.min_continuation(self.meta_continuations) {
            if let Some(IterItem { offset, .. }) =
                OffsetIter(self.offset_bytes.range(self.index..)).next()
            {
                let next_frame = self.last_run + offset;
                if next_frame < frame {
                    return Err(PastLenNotLogError::MissedUpdateForwardLog(MissedUpdate(
                        next_frame,
                    )));
                }
            }
        }

        self.meta_continuations = meta.continuations_len();

        if self.offset_bytes.len() > self.index {
            self.offset_bytes.truncate(self.index);
            self.zeroes = 0;
        }

        self.zeroes_max = self.zeroes;
        Ok(())
    }

    /// Clears the log.
    pub fn clear(&mut self) {
        self.offset_bytes.clear();
        self.out_of_or_past_end_log = 0;
        self.last_run = 0;
        self.index = 0;
        self.past_len = 0;
        self.zeroes = 0;
        self.zeroes_max = 0;
    }

    /// Update the log which does the following:
    /// - [`Self::truncate_future`] and returns its error if one or more [`Self::backward_log`]/
    ///   [`Self::forward_log`] calls were missed. See [`PastLenNotLogError`]
    /// - removes log entries before [`RevMeta::past_len`].
    /// - returns the updated length of the log as an alternative to [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::NOT_LOG`] when the current scope has been determined for
    /// some operation to happen, most often in combination with another log that is updated next
    /// with the returned value.
    ///
    /// When instead the current scope has been determined for such operations to **not** happen,
    /// [`Self::truncate_future`] **must** be called instead, next to the `drain_future` method of
    /// the accompanied log(s).
    ///
    /// Note that, if the other log is a `Transition` log and the iterator returned by its
    /// `drain_future` method is used for cleanups, the value returned here should be increased by
    /// `1` if the cleanups regard things that need to be out of log first. See the [module docs]
    /// for a more detailed explaination.
    ///
    /// See the [type docs] for an example of using this method.
    ///
    /// [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
    /// [missed log traversal updates]: MissedUpdate
    /// [module docs]: super
    /// [type docs]: PastLenLog
    pub fn update_and_get_past_len(&mut self, meta: &RevMeta, direction_changes: &DirectionChanges) -> Result<usize, PastLenNotLogError> {
        // truncate future
        self.truncate_future(meta, direction_changes)?;

        // truncate past

        let iter = OffsetIter(self.offset_bytes.iter());

        let mut to_drain = 0;

        for IterItem { offset, len } in iter {
            if offset == 0 {
                // Offsets of 0 are encoded differently, len is not the amount of bytes,
                // which is actually always 1 here, but the amount of zero offsets in this
                // one byte.
                // We want to get rid of these offsets as well as they dont bring
                // self.out_of_or_past_end_log any closer the limit.
                to_drain += 1;
                self.past_len -= len.get();
                continue;
            }

            let next_oldest = self.out_of_or_past_end_log + offset;
            if next_oldest > meta.past_end() {
                // next_oldest is reachable by log traversion, which is undesired because
                // Self::backward_log stops working there
                break;
            }

            to_drain += len.get();
            self.out_of_or_past_end_log = next_oldest;
            self.past_len -= 1;
        }

        self.index -= to_drain;
        // todo: use truncate_front when https://github.com/rust-lang/rust/issues/140667
        // stabilizes
        self.offset_bytes.drain(..to_drain);

        // push present offset
        let mut offset = meta.now() - self.last_run;
        self.last_run = meta.now();
        self.past_len += 1;

        if offset == 0 {
            // offsets of zero are not pushed right away unless the maximum is reached

            if self.zeroes == MAX_ZEROES_PER_BYTE {
                // reached the maximum amount of zeroes that fit into a single byte, push it and
                // start a new sequence of zero offsets
                self.offset_bytes.push_back(MAX_ZEROES_AS_BYTE);
                self.index += 1;
                self.zeroes = 1;
            } else {
                // increase the sequence of zero offsets
                self.zeroes += 1;
            }

            self.zeroes_max = self.zeroes;
            return Ok(self.past_len);
        } else if self.zeroes == 1 {
            // there was an offset of 0 previously, push it now that it is sure no more such offsets
            // are following it
            self.offset_bytes.push_back(0);
            self.index += 1;
        } else if self.zeroes > 1 {
            // there was a sequence of offsets of 0 previously, push it now that it is sure no more
            // such offsets are following it
            self.offset_bytes.push_back((self.zeroes - 2) | ZEROES_OR);
            self.index += 1;
        }

        self.index += 1;
        self.zeroes = 0;
        self.zeroes_max = 0;

        if offset <= MAX_SINGLE_BYTE_OFFSET as u64 {
            self.offset_bytes.push_back(offset as u8);
            return Ok(self.past_len);
        }

        // this is a multi-byte offset

        let wrapping_byte = (offset & WRAPPING_OFFSET_MASK as u64) as u8 | WRAPPING_OFFSET_OR;
        self.offset_bytes.push_back(wrapping_byte);

        // wrapping bytes contain 6 usable bits for the offset
        offset >>= 6;

        loop {
            self.index += 1;
            if offset <= MAX_WRAPPING_OFFSET as u64 {
                // this is a wrapping byte

                self.offset_bytes
                    .push_back(offset as u8 | WRAPPING_OFFSET_OR);
                return Ok(self.past_len);
            }

            // this is a wrapped byte

            self.offset_bytes
                .push_back((offset & WRAPPED_OFFSET_MASK as u64) as u8);

            // wrapped bytes contain 7 usable bits for the offset
            offset >>= 7;
        }
    }

    /// Checks at [`RevDirection::BackwardLog`] if this log has been updated at this frame.
    ///
    /// Returns `Ok(true)` if that is the case or `Ok(false)` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `Ok(false)` then as well.
    ///
    /// If this returns an error, an update has been missed, the log has been constructed from an
    /// invalid state or [`RevMeta`] is in an invalid state. See [`PastLenBackwardError`].
    ///
    /// If this log is potenitally updated more than once per frame, use this method in the fitting
    /// amount of `if` cases or with a `while` loop.
    ///
    /// See the [type docs] for an example.
    ///
    /// [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
    /// [type docs]: PastLenLog
    pub fn backward_log(&mut self, meta: &RevMeta, direction_changes: &DirectionChanges) -> Result<bool, PastLenBackwardError> {
        match self.last_run.cmp(&(meta.now() + 1)) {
            CmpOrdering::Less => Ok(false),
            CmpOrdering::Equal => {
                if self.zeroes > 0 {
                    self.zeroes -= 1;
                    self.past_len -= 1;
                    return Ok(true);
                }
                match OffsetIter(self.offset_bytes.range(..self.index)).next_back() {
                    Some(IterItem { offset: 0, len }) => {
                        self.index -= 1;
                        self.past_len -= 1;
                        self.zeroes = len.get() as u8 - 1;
                        Ok(true)
                    }
                    Some(IterItem { offset, len }) => {
                        self.last_run -= offset;
                        self.index -= len.get();
                        self.past_len -= 1;
                        self.zeroes = 0;
                        Ok(true)
                    }
                    None => Err(OutOfLog)?,
                }
            }
            CmpOrdering::Greater => Err(MissedUpdate(self.last_run))?,
        }
    }

    /// Checks at [`RevDirection::FORWARD_LOG`] if this log has been updated at this frame.
    ///
    /// Returns `Ok(true)` if that is the case or `Ok(false)` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `Ok(false)` then as well.
    ///
    /// If this returns an error, an update has been missed. See [`MissedUpdate`].
    ///
    /// If this log is potenitally updated more than once per frame, use this method in the fitting
    /// amount of `if` cases or with a `while` loop.
    ///
    /// See the [type docs] for an example.
    ///
    /// [`RevDirection::FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
    /// [type docs]: PastLenLog
    pub fn forward_log(&mut self, meta: &RevMeta, direction_changes: &DirectionChanges) -> Result<bool, MissedUpdate> {
        match OffsetIter(self.offset_bytes.range(self.index..)).next() {
            Some(IterItem { offset: 0, len }) => match self.last_run.cmp(&meta.now()) {
                CmpOrdering::Greater => Ok(false),
                CmpOrdering::Equal => {
                    if self.zeroes < len.get() as u8 - 1 {
                        self.zeroes += 1;
                    } else {
                        self.index += 1;
                        self.zeroes = 0;
                    }
                    self.past_len += 1;
                    Ok(true)
                }
                CmpOrdering::Less => Err(MissedUpdate(self.last_run)),
            },
            Some(IterItem { offset, len }) => {
                let frame = self.last_run + offset;
                match frame.cmp(&meta.now()) {
                    CmpOrdering::Greater => Ok(false),
                    CmpOrdering::Equal => {
                        self.last_run = frame;
                        self.index += len.get();
                        self.past_len += 1;
                        self.zeroes = 0;
                        Ok(true)
                    }
                    CmpOrdering::Less => Err(MissedUpdate(frame)),
                }
            }
            None if self.zeroes < self.zeroes_max => match self.last_run.cmp(&meta.now()) {
                CmpOrdering::Greater => Ok(false),
                CmpOrdering::Equal => {
                    self.past_len += 1;
                    self.zeroes += 1;
                    Ok(true)
                }
                CmpOrdering::Less => Err(MissedUpdate(self.last_run)),
            },
            None => Ok(false),
        }
    }
}

#[derive(Resource, Debug)]
pub(crate) struct DirectionChanges {
    log: VecDeque<DirectionChange>,
    present: DirectionChange,
    truncated: usize,
}

type ErrLocation = MaybeLocation<AtomicPtr<Location<'static>>>;

#[derive(Debug)]
struct DirectionChange {
    start: u64,
    direction: RevDirection,
    seen: AtomicU32,
    /// Because of no general support for AtomicU64 on all possible targets, this is an offset
    /// from [`Self::start`] instead. This also means the max global log size is
    /// `i32::MIN.unsigned_abs() as u64 + 1`.
    backward_err_limit_offset: AtomicI32,
    backward_err_location: ErrLocation,
    forward_err_limit_offset: AtomicU32,
    forward_err_location: ErrLocation,
}

impl DirectionChanges {
    pub(crate) fn new(now: u64, direction: RevDirection) -> Self {
        let err_location = || MaybeLocation::new_with(|| {
            AtomicPtr::new(
                (Location::caller() as *const Location).cast_mut()
            )
        });
        Self { 
            log: VecDeque::new(),
            present: DirectionChange { 
                start: now, 
                direction,
                seen: AtomicU32::new(0), 
                backward_err_limit_offset: AtomicI32::new(i32::MAX),
                backward_err_location: err_location(),
                forward_err_limit_offset: AtomicU32::new(u32::MIN),
                forward_err_location: err_location()
            },
            truncated: 0,
        }
    }
    pub(crate) fn update(&mut self, meta: &RevMeta) -> Result<(), ()> {

        let mut to_truncate = 0;
        for change in self.log.iter_mut() {
            if *change.seen.get_mut() != 0 {
                break;
            }

            let backward_err_limit_offset = *change.backward_err_limit_offset.get_mut();
            let backward_err_limit = if backward_err_limit_offset < 0 {
                change.start - backward_err_limit_offset.unsigned_abs() as u64
            } else {
                change.start + backward_err_limit_offset as u64
            };

            if meta.now() < backward_err_limit {
                return Err(());
            }
            if meta.past_end() > backward_err_limit { // <= ?
                // cannot go that far backward to trigger an error
                to_truncate += 1;
                continue;
            }

            if change.direction == RevDirection::NOT_LOG {
                // no FORWARD_LOG errors can be triggered if PastLenLog s could not have a future
                // in this change's point of time
                break;
            }

            let forward_err_limit_offset = *change.forward_err_limit_offset.get_mut();
            let forward_err_limit = change.start + forward_err_limit_offset as u64;
            
            // todo: remaining checks

            to_truncate += 1;
        }
        self.log.drain(..to_truncate);
        self.truncated += to_truncate;

        if direction != self.present.direction {
            let previous = core::mem::replace(&mut self.present, DirectionChange { 
                seen: AtomicU32::new(0), 
                start: now, 
                direction
            });
            self.log.push_back(previous)
        }
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
            0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101,
        ];

        // 0b0xxxxxxx = x offset including zero
        // 0b10xxxxxx = x amount of zeroes + 1
        // 0b11xxxxxx = padding byte with x payload, wraps 0b0xxxxxxx bytes with x payload

        let deque: VecDeque<u8> = [
            // 0b000000
            0b0_0000000,
            //
            // 0b000010_000001
            0b11_000001,
            0b11_000010,
            //
            // 0b000101_0000100_000011
            0b11_000011,
            0b0_0000100,
            0b11_000101,
            //
            // 0b001001_0001000_0000111_000110
            0b11_000110,
            0b0_0000111,
            0b0_0001000,
            0b11_001001,
            //
            // 0b001110_0001101_0001100_0001011_001010
            0b11_001010,
            0b0_0001011,
            0b0_0001100,
            0b0_0001101,
            0b11_001110,
            //
            // 0b010100_0010011_0010010_0010001_0010000_001111
            0b11_001111,
            0b0_0010000,
            0b0_0010001,
            0b0_0010010,
            0b0_0010011,
            0b11_010100,
            //
            // 0b011011_0011010_0011001_0011000_0010111_0010110_010101
            0b11_010101,
            0b0_0010110,
            0b0_0010111,
            0b0_0011000,
            0b0_0011001,
            0b0_0011010,
            0b11_011011,
            //
            // 0b100011_0100010_0100001_0100000_0011111_0011110_0011101_011100
            0b11_011100,
            0b0_0011101,
            0b0_0011110,
            0b0_0011111,
            0b0_0100000,
            0b0_0100001,
            0b0_0100010,
            0b11_100011,
            //
            // 0b101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100
            0b11_100100,
            0b0_0100101,
            0b0_0100110,
            0b0_0100111,
            0b0_0101000,
            0b0_0101001,
            0b0_0101010,
            0b0_0101011,
            0b11_101100,
            //
            // 0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101
            0b11_101101,
            0b0_0101110,
            0b0_0101111,
            0b0_0110000,
            0b0_0110001,
            0b0_0110010,
            0b0_0110011,
            0b0_0110100,
            0b0_0110101,
            0b11_0000_10, // only least significant two bits are available, more would overflow u64
            //
            // two following zeroes
            0b10_000000,
            //
            // 65 following zeroes
            MAX_ZEROES_AS_BYTE,
        ]
        .into();

        fn item(offset: u64, len: usize) -> IterItem {
            IterItem {
                offset,
                len: NonZeroUsize::new(len).unwrap(),
            }
        }

        let expected = [
            // single byte value
            item(offsets[0], 1),
            // multi byte values
            item(offsets[1], 2),
            item(offsets[2], 3),
            item(offsets[3], 4),
            item(offsets[4], 5),
            item(offsets[5], 6),
            item(offsets[6], 7),
            item(offsets[7], 8),
            item(offsets[8], 9),
            item(offsets[9], 10),
            // 2 zeroes in a byte
            item(0, 2),
            // 65 zeroes in a byte
            item(0, MAX_ZEROES_PER_BYTE as usize),
        ];

        assert!(OffsetIter(deque.iter()).eq(expected.iter().cloned()));

        assert!(
            OffsetIter(deque.iter())
                .rev()
                .eq(expected.into_iter().rev())
        );
    }

    #[derive(Clone)]
    struct Log {
        log: PastLenLog,
        meta: RevMeta,
    }

    impl Log {
        fn new(max_world_states: u64, now: u64) -> Self {
            let log = PastLenLog::new();
            let meta = RevMeta::new(NonZeroU64::new(max_world_states), now, false);
            Self { log, meta }
        }
        fn forward(
            &mut self,
            updates_with_expected_past_len: Result<Vec<usize>, PastLenNotLogError>,
        ) {
            self.meta.queue_not_log_forward();
            self.meta
                .update(|meta| match updates_with_expected_past_len {
                    Ok(updates_with_expected_past_len) => {
                        let len = updates_with_expected_past_len.len();
                        let mut before = Vec::with_capacity(len);
                        let updates_with_actual_past_len: Vec<usize> = (0..len)
                            .map(|_| {
                                before.push(self.log.clone());
                                match self.log.update_and_get_past_len(meta) {
                                    Ok(past_len) => past_len,
                                    Err(err) => panic!(
                                        "{err}\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                                        self.log
                                    ),
                                }
                            })
                            .collect();
                        assert_eq!(
                            updates_with_actual_past_len, updates_with_expected_past_len,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        )
                    }
                    Err(err) => {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.update_and_get_past_len(&meta),
                            Err(err),
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        );
                    }
                })
                .expect("should update");
        }
        fn forward_log(&mut self, expected_backward_log_updates: Result<usize, MissedUpdate>) {
            let previous = self.meta.now() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| match expected_backward_log_updates {
                    Ok(expected_backward_log_updates) => {
                        let mut before = Vec::with_capacity(expected_backward_log_updates + 1);
                        for _ in 0..expected_backward_log_updates {
                            before.push(self.log.clone());
                            assert_eq!(
                                self.log.forward_log(meta),
                                Ok(true),
                                "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                                self.log
                            );
                        }
                        before.push(self.log.clone());
                        assert_eq!(
                            self.log.forward_log(meta),
                            Ok(false),
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        );
                    }
                    Err(missed_frame) => {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.forward_log(meta),
                            Err(missed_frame),
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        );
                    }
                })
                .expect("should update");
        }
        fn forward_log_miss_frame(&mut self) {
            let previous = self.meta.now() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta.update(|_| {}).expect("should update");
        }
        fn backward_log(&mut self, expected_backward_log_updates: Result<usize, MissedUpdate>) {
            let previous = self.meta.now() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| match expected_backward_log_updates {
                    Ok(expected_backward_log_updates) => {
                        let mut before = Vec::with_capacity(expected_backward_log_updates + 1);
                        for _ in 0..expected_backward_log_updates {
                            before.push(self.log.clone());
                            assert_eq!(
                                self.log.backward_log(meta),
                                Ok(true),
                                "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                                self.log
                            );
                        }
                        before.push(self.log.clone());
                        assert_eq!(
                            self.log.backward_log(meta),
                            Ok(false),
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        );
                    }
                    Err(missed_frame) => {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.backward_log(meta),
                            Err(PastLenBackwardError::MissedUpdate(missed_frame)),
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:#?}",
                            self.log
                        );
                    }
                })
                .expect("should update");
        }
        fn backward_log_miss_frame(&mut self) {
            let previous = self.meta.now() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta.update(|_| {}).expect("should update");
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut log = Log::new(4, 0);
        log.forward(Ok(vec![1])); // frame #1
        log.forward(Ok(vec![2, 3])); // frame #2
        log.forward(Ok(vec![4, 5]));
        log.forward(Ok(vec![]));
        // shortened log of runs from frame #1 and #2 --> past_len -= 3
        log.forward(Ok(vec![3, 4, 5]));

        log.backward_log(Ok(3));
        log.backward_log(Ok(0));
        log.backward_log(Ok(2));

        log.forward_log(Ok(2));
        log.forward_log(Ok(0));
        log.forward_log(Ok(3));

        log.backward_log(Ok(3));
        log.backward_log(Ok(0));
        log.backward_log(Ok(2));

        log.forward(Ok(vec![1]));
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut log = Log::new(4, 0);

        log.forward(Ok(vec![1])); // frame #1
        assert_eq!(log.meta.past_len(), 1);

        log.forward(Ok(vec![2])); // frame #2
        assert_eq!(log.meta.past_len(), 2);

        log.forward(Ok(vec![3]));
        assert_eq!(log.meta.past_len(), 3);

        log.forward(Ok(vec![3]));
        assert_eq!(log.meta.past_len(), 3);
    }

    #[test]
    fn missed_update_in_log() {
        let mut log = Log::new(4, 0);

        log.forward(Ok(vec![1])); // frame #1
        log.forward(Ok(vec![2])); // frame #2
        log.forward(Ok(vec![3])); // frame #3

        {
            let mut log = log.clone();
            let err = MissedUpdate(3);
            log.backward_log_miss_frame();
            log.backward_log(Err(err));
            log.forward(Err(PastLenNotLogError::MissedUpdateBackwardLog(err)));
        }

        log.backward_log(Ok(1));
        log.backward_log(Ok(1));
        log.backward_log(Ok(1));

        // -- log edge --
        let err = MissedUpdate(1);
        log.forward_log_miss_frame();
        log.forward_log(Err(err));
        log.forward(Err(PastLenNotLogError::MissedUpdateForwardLog(err)));
    }

    #[test]
    fn missed_update_out_of_log() {
        let mut log = Log::new(3, 0);

        log.forward(Ok(vec![1]));

        log.backward_log(Ok(1));
        log.forward_log_miss_frame();

        log.forward(Ok(vec![]));

        // -- log edge --
        log.forward(Ok(vec![]));
        log.forward(Ok(vec![]));

        log.forward(Err(PastLenNotLogError::MissedUpdateForwardLog(
            MissedUpdate(1),
        )));
    }

    #[test]
    fn no_missed_frame_false_positive_in_log() {
        let mut log = Log::new(3, 0);

        log.forward(Ok(vec![1]));

        log.backward_log(Ok(1));

        log.forward(Ok(vec![]));
        // should not detect a missed forward log update
        log.forward(Ok(vec![1]));
    }

    #[test]
    fn no_missed_frame_false_positive_out_of_log() {
        let mut log = Log::new(3, 0);

        log.forward(Ok(vec![1]));

        log.backward_log(Ok(1));

        log.forward(Ok(vec![]));

        // -- log edge --
        log.forward(Ok(vec![]));
        log.forward(Ok(vec![]));
        // should not detect a missed forward log update
        log.forward(Ok(vec![1]));
    }
}
