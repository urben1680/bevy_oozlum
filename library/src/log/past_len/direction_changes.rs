use std::{
    collections::VecDeque,
    panic::Location,
    sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, Ordering::SeqCst},
};

use bevy::ecs::{change_detection::MaybeLocation, resource::Resource};

use crate::meta::{RevDirection, RevMeta};

const DEFAULT_LOCATION: &'static Location<'static> = Location::caller();
const DEFAULT_MAYBE_LOCAION: MaybeLocation = MaybeLocation::new(DEFAULT_LOCATION);

#[derive(Debug)]
struct AtomicLocation(MaybeLocation<AtomicPtr<Location<'static>>>);

impl Default for AtomicLocation {
    fn default() -> Self {
        Self(MaybeLocation::new_with(|| {
            AtomicPtr::new((DEFAULT_LOCATION as *const Location).cast_mut())
        }))
    }
}

impl AtomicLocation {
    fn swap(&self, location: MaybeLocation) -> MaybeLocation {
        location.zip(self.0.as_ref()).map(|(location, this)| {
            let mut ptr_const = location as *const Location;
            let mut ptr_mut = ptr_const as *mut Location;
            ptr_mut = this.swap(ptr_mut, SeqCst);
            ptr_const = ptr_mut as *const Location;
            unsafe {
                // SAFETY: contained reference came from Location::caller which is static
                &*ptr_const
            }
        })
    }
    fn swap_mut(&mut self, location: MaybeLocation) -> MaybeLocation {
        location.zip(self.0.as_mut()).map(|(location, this)| {
            let mut ptr_const = location as *const Location;
            let mut ptr_mut = ptr_const as *mut Location;
            core::mem::swap(this.get_mut(), &mut ptr_mut);
            ptr_const = ptr_mut as *const Location;
            unsafe {
                // SAFETY: contained reference came from Location::caller which is static
                &*ptr_const
            }
        })
    }
    fn get(&mut self) -> MaybeLocation {
        self.0.as_mut().map(|ptr| {
            let ptr_mut = *ptr.get_mut();
            let ptr_const = ptr_mut.cast_const();
            unsafe {
                // SAFETY: contained reference came from Location::caller which is static
                &*ptr_const
            }
        })
    }
}

const FORWARD_OFFSET_DEFAULT: u32 = BACKWARD_OFFSET_DEFAULT.unsigned_abs();
const BACKWARD_OFFSET_DEFAULT: i32 = i32::MIN;
pub(crate) const MAX_LOG_LEN: u64 = FORWARD_OFFSET_DEFAULT as u64;

#[derive(Debug)]
struct DirectionChangeSwap<Offset, Location> {
    offset: Offset,
    location: Location,
}

impl Default for DirectionChangeSwap<u32, MaybeLocation> {
    fn default() -> Self {
        Self {
            offset: FORWARD_OFFSET_DEFAULT,
            location: DEFAULT_MAYBE_LOCAION,
        }
    }
}

impl Default for DirectionChangeSwap<i32, MaybeLocation> {
    fn default() -> Self {
        Self {
            offset: BACKWARD_OFFSET_DEFAULT,
            location: DEFAULT_MAYBE_LOCAION,
        }
    }
}

impl Default for DirectionChangeSwap<AtomicU32, AtomicLocation> {
    fn default() -> Self {
        Self {
            offset: AtomicU32::new(FORWARD_OFFSET_DEFAULT),
            location: Default::default(),
        }
    }
}

impl Default for DirectionChangeSwap<AtomicI32, AtomicLocation> {
    fn default() -> Self {
        Self {
            offset: AtomicI32::new(BACKWARD_OFFSET_DEFAULT),
            location: Default::default(),
        }
    }
}

trait StateOption {
    /// Does not mutate value, but enables non-atomic read.
    fn is_default(&mut self) -> bool;
}

impl StateOption for DirectionChangeSwap<u32, MaybeLocation> {
    fn is_default(&mut self) -> bool {
        self.offset == FORWARD_OFFSET_DEFAULT
    }
}

impl StateOption for DirectionChangeSwap<i32, MaybeLocation> {
    fn is_default(&mut self) -> bool {
        self.offset == BACKWARD_OFFSET_DEFAULT
    }
}

impl StateOption for DirectionChangeSwap<AtomicU32, AtomicLocation> {
    fn is_default(&mut self) -> bool {
        *self.offset.get_mut() == FORWARD_OFFSET_DEFAULT
    }
}

impl StateOption for DirectionChangeSwap<AtomicI32, AtomicLocation> {
    fn is_default(&mut self) -> bool {
        *self.offset.get_mut() == BACKWARD_OFFSET_DEFAULT
    }
}

#[derive(Debug)]
struct DirectionChange {
    start: u64,
    direction: RevDirection,
    /// Because of no general support for AtomicU64 on all possible targets, this is an offset
    /// from [`Self::start`] instead.
    ///
    /// As the limit may come before or after `start`, this needs to be an `i32`.
    ///
    /// This also means the max global log size is limited to `i32::MIN.unsigned_abs() + 1`.
    /// If this limit was exceeded, frames at which an error would occur could not be expressed
    /// here and thus not be detected.
    backward: DirectionChangeSwap<AtomicI32, AtomicLocation>, // None: kann niemals u32::MAX haben
    forward: DirectionChangeSwap<AtomicU32, AtomicLocation>, // None: kann niemals offset i32::MIN haben
}

impl DirectionChange {
    fn new(meta: &RevMeta) -> Self {
        Self {
            start: meta.now(),
            direction: meta.running_direction(),
            forward: Default::default(),
            backward: Default::default(),
        }
    }

    fn check_backward(&mut self, meta: &RevMeta) -> Result<u64, MaybeLocation> {
        let offset = *self.backward.offset.get_mut();
        let frame = if offset < 0 {
            self.start - offset.unsigned_abs() as u64
        } else {
            self.start + offset as u64
        };

        if meta.now() < frame {
            return Err(self.backward.location.get());
        }

        Ok(frame)
    }

    fn check_forward(&mut self, meta: &RevMeta) -> Result<u64, MaybeLocation> {
        let offset = *self.forward.offset.get_mut();
        let frame = self.start + offset as u64;

        if meta.now() > frame {
            return Err(self.forward.location.get());
        }

        Ok(frame)
    }
}

#[derive(Resource, Debug)]
pub struct DirectionChanges {
    log: VecDeque<DirectionChange>,
    present: DirectionChange,
    truncated: usize,
}

impl DirectionChanges {
    pub(crate) fn new(meta: &RevMeta) -> Self {
        Self {
            log: VecDeque::new(),
            present: DirectionChange::new(meta),
            truncated: 0,
        }
    }
    pub(crate) fn update(&mut self, meta: &RevMeta) -> Result<(), MaybeLocation> {
        let mut to_truncate = 0;

        // mutable iter items enable non-atomic reads but nothing is actually mutated
        let mut iter = self.log.iter_mut();

        // check if any offset has been breached or if they can be truncated as out-of-log
        for change in iter.by_ref() {
            let backward_infallible =
                change.backward.is_default() || meta.past_end() > change.check_backward(meta)?;

            // always true at change.direction == RevDirection::NOT_LOG as it remains default
            let forward_infallible =
                change.forward.is_default() || meta.past_end() > change.check_forward(meta)?;

            // do not further inline to ensure both fallible checks always run
            if !backward_infallible || !forward_infallible {
                break;
            }

            to_truncate += 1;
        }

        // check if any remaining offset has been breached
        for change in iter {
            if !change.backward.is_default() {
                change.check_backward(meta)?;
            }
            if !change.forward.is_default() {
                change.check_forward(meta)?;
            }
        }

        // truncate changes that cannot have their offsets breached anymore
        self.log.drain(..to_truncate);
        self.truncated += to_truncate;

        if self.present.start + MAX_LOG_LEN <= meta.now()
            || meta.running_direction() != self.present.direction
        {
            let previous = core::mem::replace(&mut self.present, DirectionChange::new(meta));
            self.log.push_back(previous);
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct DirectionChangeState {
    index: usize,
    backward: Option<DirectionChangeSwap<i32, MaybeLocation>>,
    forward: Option<DirectionChangeSwap<u32, MaybeLocation>>,
}

impl DirectionChangeState {
    /// Only call this at offsets != 0
    #[track_caller]
    pub(super) fn update(
        &mut self,
        changes: &DirectionChanges,
        past_limit: Option<u64>,
        future_limit: Option<u64>)
    {
        if self.backward.is_some() || self.forward.is_some() {
            let index = self
                .index
                .checked_sub(changes.truncated)
                .filter(|index| *index + 1 < changes.log.len());
            if let Some(index) = index {
                let change = &changes.log[index];

                if let Some(swap) = self.backward.take() {
                    let change = &change.backward;
                    let offset = swap.offset;
                    if change.offset.fetch_max(offset, SeqCst) > offset {
                        change.location.swap(swap.location);
                    }
                }
                
                if let Some(swap) = self.forward.take() {
                    let change = &change.forward;
                    let offset = swap.offset;
                    if change.offset.fetch_min(offset, SeqCst) < offset {
                        change.location.swap(swap.location);
                    }
                }
            }
        }

        let start = changes.present.start;

        if let Some(past_limit) = past_limit {
            let present = &changes.present.backward;

            let offset = if past_limit < start {
                let abs = start - past_limit;
                -(abs as i32)
            } else {
                let abs = past_limit - start;
                abs as i32
            };

            let previous = present.offset.fetch_max(offset, SeqCst);

            if previous < offset {
                self.backward = Some(DirectionChangeSwap {
                    offset: previous,
                    location: present.location.swap(MaybeLocation::caller())
                });

                self.index = changes.log.len() + changes.truncated;
            }
        }

        if let Some(future_limit) = future_limit {
            let present = &changes.present.forward;

            let offset = (future_limit - start) as u32;

            let previous = present.offset.fetch_min(offset, SeqCst);

            if previous > offset {
                self.forward = Some(DirectionChangeSwap {
                    offset: previous,
                    location: present.location.swap(MaybeLocation::caller())
                });

                self.index = changes.log.len() + changes.truncated;
            }
        }
    }

    fn update_mut(
        &mut self,
        changes: &mut DirectionChanges,
        past_limit: Option<u64>,
        future_limit: Option<u64>
    ) {
        if self.backward.is_some() || self.forward.is_some() {
            let index = self
                .index
                .checked_sub(changes.truncated)
                .filter(|index| *index + 1 < changes.log.len());
            if let Some(index) = index {
                let change = &mut changes.log[index];

                if let Some(swap) = self.backward.take() {
                    let change = &mut change.backward;
                    let offset = swap.offset;
                    let previous = change.offset.get_mut();
                    if *previous > offset {
                        *previous = offset;
                        change.location.swap_mut(swap.location);
                    }
                }
                
                if let Some(swap) = self.forward.take() {
                    let change = &mut change.forward;
                    let offset = swap.offset;
                    let previous = change.offset.get_mut();
                    if *previous < offset {
                        *previous = offset;
                        change.location.swap_mut(swap.location);
                    }
                }
            }
        }

        let start = changes.present.start;

        if let Some(past_limit) = past_limit {
            let present = &mut changes.present.backward;

            let offset = if past_limit < start {
                let abs = start - past_limit;
                -(abs as i32)
            } else {
                let abs = past_limit - start;
                abs as i32
            };

            let previous = present.offset.get_mut();

            if *previous < offset {
                self.backward = Some(DirectionChangeSwap {
                    offset: *previous,
                    location: present.location.swap_mut(MaybeLocation::caller())
                });

                *previous = offset;

                self.index = changes.log.len() + changes.truncated;
            }
        }

        if let Some(future_limit) = future_limit {
            let present = &mut changes.present.forward;

            let offset = (future_limit - start) as u32;

            let previous = present.offset.get_mut();

            if *previous > offset {
                self.forward = Some(DirectionChangeSwap {
                    offset: *previous,
                    location: present.location.swap_mut(MaybeLocation::caller())
                });

                *previous = offset;

                self.index = changes.log.len() + changes.truncated;
            }
        }
    }
}

/*
test Dimensionen:

1. past_limit negativ, positiv
2. future_limit vor wechsel, nach wechsel

Problem?

frame                | 1 2 
changes future limit | 9 5
log1 future limit    |   5
log2 future limit    |   6

log 1 gewinnt bei forward das rennen und führt swap aus

log 2 wird nicht geupdated aber das wird nicht erkannt weil dessen limit bei undo nicht in changes landet

muss wohl doch alles geloggt werden
*/
