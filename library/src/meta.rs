use core::{num::NonZeroUsize, ops::Range};

use bevy::{
    ecs::{schedule::ScheduleLabel, system::Resource, world::World},
    reflect::{std_traits::ReflectDefault, Reflect},
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{app::TryRunRevScheduleError, log::WithTimestamp};
use crate::{BackwardSchedule, ForwardSchedule, RevUpdate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub enum Direction {
    Forward,
    ForwardLog { updates_until_pause: NonZeroUsize },
    BackwardLog { updates_until_pause: NonZeroUsize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub enum InternalDirection {
    RunningForward,
    RunningForwardLog { updates_until_pause: NonZeroUsize },
    RunningBackwardLog { updates_until_pause: NonZeroUsize },
    RanForward,
    RanForwardLog { updates_until_pause: NonZeroUsize },
    RanBackwardLog { updates_until_pause: NonZeroUsize },
    Pause,
}

impl InternalDirection {
    fn running_from_queue(queue: Option<Direction>) -> Self {
        match queue {
            Some(Direction::Forward) => Self::RunningForward,
            Some(Direction::ForwardLog {
                updates_until_pause,
            }) => Self::RunningForwardLog {
                updates_until_pause,
            },
            Some(Direction::BackwardLog {
                updates_until_pause,
            }) => Self::RunningBackwardLog {
                updates_until_pause,
            },
            None => Self::Pause,
        }
    }
    fn set_running(&mut self) {
        *self = match *self {
            Self::RanForward => Self::RunningForward,
            Self::RanForwardLog {
                updates_until_pause,
            } => Self::RunningForwardLog {
                updates_until_pause,
            },
            Self::RanBackwardLog {
                updates_until_pause,
            } => Self::RunningBackwardLog {
                updates_until_pause,
            },
            _ => *self,
        }
    }
    fn set_ran(&mut self) {
        *self = match *self {
            Self::RunningForward => Self::RanForward,
            Self::RunningForwardLog {
                updates_until_pause,
            } => Self::RanForwardLog {
                updates_until_pause,
            },
            Self::RunningBackwardLog {
                updates_until_pause,
            } => Self::RanBackwardLog {
                updates_until_pause,
            },
            _ => *self,
        }
    }
    pub fn get_direction(self) -> Option<Direction> {
        match self {
            Self::RunningForward => Some(Direction::Forward),
            Self::RunningForwardLog {
                updates_until_pause,
            } => Some(Direction::ForwardLog {
                updates_until_pause,
            }),
            Self::RunningBackwardLog {
                updates_until_pause,
            } => Some(Direction::BackwardLog {
                updates_until_pause,
            }),
            _ => None,
        }
    }
    pub fn running_rev_schedule(self) -> bool {
        matches!(
            self,
            Self::RunningForward | Self::RunningForwardLog { .. } | Self::RunningBackwardLog { .. }
        )
    }
}

/*
benötigte Werte in hot code:
- now
- log_start
- max_updates (über Direction Eager, user geben ein impl TryInto<usize>, kein usize oder NonZeroUsize)

benötigt für steuerung:
- log end
*/

/// RevMeta is used to control the processing of reversible systems.
///
/// It keepts track what the current frame is and to which frame one can go forward and backward in time.
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct RevMeta {
    /// The maximum amount of states of the world that is logged to be jumped in, or None if growth is unrestricted.
    ///
    /// As the world is always in a certain state, the amount cannot be zero.
    ///
    /// World states that become too old will no longer be accessible after an update, even if raising this value afterwards.
    /// If one wants to keep a certain frame accessible, one needs to _either_:
    /// - regularily set this value to not less than `now() + 2 - frame` before the next update
    /// - set it to `None`, disabling forgetting world states
    ///
    /// Reducing this value alone does not cause deallocations, this has to be done manually with each [`crate::log`] struct if desired.
    ///
    /// Changing this value is always possible but only comes into effect when updating the world during [`Direction::Forward`].
    pub max_len: Option<NonZeroUsize>,
    now: usize,
    range: Range<usize>,
    queue: Option<Option<Direction>>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroUsize::MIN), 0, false)
    }
}

impl RevMeta {
    const END_MAX_MSG: &'static str = "Maximum reversible timestamp reached: `usize::MAX`";
    pub(crate) const EXIST_MSG: &'static str = "The RevMeta resource should never be removed";
    pub const fn new(max_len: Option<NonZeroUsize>, now: usize, paused: bool) -> Self {
        if now == usize::MAX {
            panic!("{}", Self::END_MAX_MSG);
        }
        Self {
            max_len,
            now,
            range: now..now + 1,
            direction: match paused {
                true => InternalDirection::Pause,
                false => InternalDirection::RanForward,
            },
            queue: None,
        }
    }
    pub fn direction(&self) -> Direction {
        self.get_direction().expect("todo")
    }
    pub fn get_direction(&self) -> Option<Direction> {
        self.direction.get_direction()
    }
    pub fn internal_direction(&self) -> InternalDirection {
        self.direction
    }
    pub fn running_rev_schedule(&self) -> bool {
        self.direction.running_rev_schedule()
    }
    pub fn now(&self) -> usize {
        self.now
    }
    pub fn past_len(&self) -> usize {
        self.now - self.range.start
    }
    pub fn with_timestamp<T>(&self, data: T) -> WithTimestamp<T> {
        WithTimestamp {
            value: data,
            logged_at: self.now.into(),
        }
    }
    /// Returns the frame range that can be returned to using [`Self::queue_log`].
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
    pub fn reduce_range(&mut self, range: Range<usize>) -> Result<(), ()> {
        if range.start >= self.range.start
            && range.end <= self.range.end
            && range.contains(&self.now)
        {
            self.range = range;
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn clear(&mut self) {
        self.range = self.now..self.now + 1;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(Some(Direction::Forward));
    }
    pub fn queue_log(&mut self, to: usize) -> Result<usize, Range<usize>> {
        if !self.range().contains(&to) {
            return Err(self.range());
        }
        let updates_until_pause = to.abs_diff(self.now);
        self.queue = Some(NonZeroUsize::new(updates_until_pause).map(
            |updates_until_pause| match to > self.now {
                true => Direction::ForwardLog {
                    updates_until_pause,
                },
                false => Direction::BackwardLog {
                    updates_until_pause,
                },
            },
        ));
        Ok(updates_until_pause)
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(None);
    }
    pub fn update_world(world: &mut World) {
        let mut this = world.get_resource_mut::<Self>().expect(Self::EXIST_MSG);
        if this.range.end == usize::MAX {
            todo!("")
        }
        this.update();
        let result = match this.get_direction() {
            Some(Direction::Forward) | Some(Direction::ForwardLog { .. }) => {
                world.try_run_schedule(ForwardSchedule(RevUpdate.intern()))
            }
            Some(Direction::BackwardLog { .. }) => {
                world.try_run_schedule(BackwardSchedule(RevUpdate.intern()))
            }
            None => return,
        };
        world
            .get_resource_mut::<Self>()
            .expect(Self::EXIST_MSG)
            .direction
            .set_ran();
        if result.is_err() {
            todo!("")
        }
    }
    pub fn update(&mut self) {
        match self.queue.take() {
            Some(queue) => {
                self.direction = InternalDirection::running_from_queue(queue);
                match self.get_direction() {
                    Some(Direction::Forward) => self.update_forward(),
                    Some(Direction::ForwardLog { .. }) => self.now += 1,
                    Some(Direction::BackwardLog { .. }) => self.now -= 1,
                    None => {}
                }
            }
            None => {
                self.direction.set_running();
                match &mut self.direction {
                    InternalDirection::RunningForward => self.update_forward(),
                    InternalDirection::RunningForwardLog {
                        updates_until_pause,
                    } => match reduction_successful(updates_until_pause) {
                        true => self.now += 1,
                        false => self.direction = InternalDirection::Pause,
                    },
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => match reduction_successful(updates_until_pause) {
                        true => self.now -= 1,
                        false => self.direction = InternalDirection::Pause,
                    },
                    _ => {}
                }
            }
        }
    }
    fn update_forward(&mut self) {
        self.now += 1;
        self.range.end = self.now.checked_add(1).expect(Self::END_MAX_MSG);
        if let Some(max_len) = self.max_len {
            self.range.start = self
                .range
                .start
                .max(self.range.end.saturating_sub(max_len.get()));
        }
    }
    pub fn run_rev_schedule(&self, world: &mut World, label: impl ScheduleLabel) {
        let label = label.intern();
        match self.direction() {
            Direction::Forward | Direction::ForwardLog { .. } => {
                world.run_schedule(ForwardSchedule(label))
            }
            Direction::BackwardLog { .. } => world.run_schedule(BackwardSchedule(label)),
        }
    }
    pub fn try_run_rev_schedule(
        &self,
        world: &mut World,
        label: impl ScheduleLabel,
    ) -> Result<(), TryRunRevScheduleError> {
        let label = label.intern();
        let Some(direction) = self.get_direction() else {
            return Err(TryRunRevScheduleError::MainRevScheduleNotRunning);
        };
        let result = match direction {
            Direction::Forward | Direction::ForwardLog { .. } => {
                world.try_run_schedule(ForwardSchedule(label))
            }
            Direction::BackwardLog { .. } => world.try_run_schedule(BackwardSchedule(label)),
        };
        result.map_err(|_| TryRunRevScheduleError::BevyTryRunScheduleError)
    }
}

fn reduction_successful(updates_until_pause: &mut NonZeroUsize) -> bool {
    NonZeroUsize::new(updates_until_pause.get() - 1)
        .map(|reduced| *updates_until_pause = reduced)
        .is_some()
}

#[cfg(test)]
mod test {
    use super::*;

    const ONE: NonZeroUsize = NonZeroUsize::MIN;
    const TWO: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };

    /// Constructs [`RevMeta`] and asserts the values are valid
    fn arrange(
        max_len: Option<NonZeroUsize>,
        now: usize,
        range: Range<usize>,
        direction: Option<Direction>,
    ) -> RevMeta {
        let meta = RevMeta {
            max_len,
            now,
            range: range.clone(),
            direction: InternalDirection::running_from_queue(direction),
            queue: None,
        };
        assert!(range.start <= now, "{meta:?}");
        match direction {
            Some(Direction::Forward) => assert_eq!(now, range.end + 1, "{meta:?}"),
            Some(Direction::ForwardLog {
                updates_until_pause,
            }) => {
                assert!(now < range.end, "{meta:?}");
                assert!(now + updates_until_pause.get() <= range.end, "{meta:?}");
            }
            Some(Direction::BackwardLog {
                updates_until_pause,
            }) => {
                assert!(now < range.end, "{meta:?}");
                assert!(
                    range.start + updates_until_pause.get() - 1 <= now,
                    "{meta:?}"
                );
            }
            None => assert!(now < range.end, "{meta:?}"),
        }
        meta
    }

    #[test]
    fn log_forward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            0,
            0..2,
            Some(Direction::ForwardLog {
                updates_until_pause: TWO,
            }),
        );

        meta.update();
        assert_eq!(
            meta.get_direction(),
            Some(Direction::ForwardLog {
                updates_until_pause: ONE
            })
        );
        assert_eq!(meta.now(), 1);

        meta.update();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now(), 1);
    }

    #[test]
    fn log_backward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            1,
            0..2,
            Some(Direction::BackwardLog {
                updates_until_pause: TWO,
            }),
        );

        meta.update();
        assert_eq!(
            meta.get_direction(),
            Some(Direction::BackwardLog {
                updates_until_pause: ONE
            })
        );
        assert_eq!(meta.now(), 0);

        meta.update();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now(), 0);
    }

    #[test]
    fn start_grows_according_to_max_len() {
        let mut meta = RevMeta::new(Some(TWO), 0, false);

        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.range(), 0..2);

        meta.update();
        assert_eq!(meta.now(), 2);
        assert_eq!(meta.range(), 1..3);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..4, None);

        assert_eq!(meta.queue_log(0), Err(1..4));
        assert_eq!(meta.queue_log(4), Err(1..4));
    }
}
