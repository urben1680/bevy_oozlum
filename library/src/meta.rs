use core::{num::NonZeroUsize, ops::Range};

use bevy::{
    ecs::system::Resource,
    prelude::{IntoSystem, ReadOnlySystem, Res},
};

// todo: updates in log directions einschließen?

// kein eager, das ist aufgabe von physik->render systemen die keine Rolel in der Kausalität splielen
// wie mit direction wechsel umgehen wenn eine entity bis x geskipped wird? gar nicht mehr skippen? genau, oder eben reversible command nutzen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    ForwardLog { updates_until_pause: NonZeroUsize },
    BackwardLog { updates_until_pause: NonZeroUsize },
    Pause,
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
#[derive(Debug, Clone, Resource)]
pub struct RevMeta {
    /// The maximum amount of states of the world that is logged to be jumped in.
    ///
    /// As the world is always in a certain state, the amount cannot be zero.
    ///
    /// World states that become too old will no longer be accessible after an update, even if raising this value afterwards.
    /// If one wants to keep a certain frame accessible, one needs to _either_:
    /// - regularily set this value to not less than `now() + 2 - frame` before the next update
    /// - set it to `None`, disabling forgetting world states
    ///
    /// Reducing this value alone does not cause deallocations, this has to be done manually with each log deque if desired.
    ///
    /// Changing this value is always possible but only comes into effect when updating the world during [`Direction::Forward`].
    pub max_len: Option<NonZeroUsize>,
    start: usize,
    now: usize,
    end: usize,
    direction: Direction,
    queue: Option<Direction>,
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
            start: now,
            now,
            end: now + 1,
            direction: match paused {
                true => Direction::Pause,
                false => Direction::Forward,
            },
            queue: None,
        }
    }
    pub fn direction(&self) -> Direction {
        self.direction
    }
    pub fn now(&self) -> usize {
        self.now
    }
    /// Returns the frame range that can be returned to using [`Self::queue_log`].
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(Direction::Forward);
    }
    pub fn queue_log(&mut self, to: usize) -> Result<usize, Range<usize>> {
        let log_range = self.range();
        if !log_range.contains(&to) {
            return Err(log_range);
        }
        let updates_until_pause = to.abs_diff(self.now);
        self.queue = Some(NonZeroUsize::new(updates_until_pause).map_or(
            Direction::Pause,
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
        self.queue = Some(Direction::Pause);
    }
    pub(crate) fn update(&mut self) {
        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                match self.direction {
                    Direction::Forward => self.update_forward(),
                    Direction::ForwardLog { .. } => self.now += 1,
                    Direction::BackwardLog { .. } => self.now -= 1,
                    Direction::Pause => {}
                }
            }
            None => match &mut self.direction {
                Direction::Forward => self.update_forward(),
                Direction::ForwardLog {
                    updates_until_pause,
                } => match reduction_successful(updates_until_pause) {
                    true => self.now += 1,
                    false => self.direction = Direction::Pause,
                },
                Direction::BackwardLog {
                    updates_until_pause,
                } => match reduction_successful(updates_until_pause) {
                    true => self.now -= 1,
                    false => self.direction = Direction::Pause,
                },
                Direction::Pause => {}
            },
        }
    }
    fn update_forward(&mut self) {
        self.now += 1;
        self.end = self.now.checked_add(1).expect(Self::END_MAX_MSG);
        if let Some(max_len) = self.max_len {
            self.start = self.start.max(self.end.saturating_sub(max_len.get()));
        }
    }
    pub(crate) fn noop_read_system() -> impl ReadOnlySystem {
        IntoSystem::into_system(|_: Res<Self>| {})
    }
}

fn reduction_successful(updates_until_pause: &mut NonZeroUsize) -> bool {
    match NonZeroUsize::new(updates_until_pause.get() - 1) {
        Some(reduced) => {
            *updates_until_pause = reduced;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const ONE: NonZeroUsize = NonZeroUsize::MIN;
    const TWO: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };

    /// Constructs [`RevMeta`] and asserts the values are valid
    fn arrange(
        max_len: Option<NonZeroUsize>,
        start: usize,
        now: usize,
        end_inclusive: usize,
        direction: Direction,
    ) -> RevMeta {
        let meta = RevMeta {
            max_len,
            start,
            now,
            end: end_inclusive,
            direction,
            queue: None,
        };
        assert!(start <= now, "{meta:?}");
        match direction {
            Direction::Forward => assert_eq!(now, end_inclusive, "{meta:?}"),
            Direction::ForwardLog {
                updates_until_pause,
            } => {
                assert!(now <= end_inclusive, "{meta:?}");
                assert!(
                    now + updates_until_pause.get() - 1 <= end_inclusive,
                    "{meta:?}"
                );
            }
            Direction::BackwardLog {
                updates_until_pause,
            } => {
                assert!(now <= end_inclusive, "{meta:?}");
                assert!(start + updates_until_pause.get() - 1 <= now, "{meta:?}");
            }
            Direction::Pause => assert!(now <= end_inclusive, "{meta:?}"),
        }
        meta
    }

    #[test]
    fn log_forward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            0,
            0,
            1,
            Direction::ForwardLog {
                updates_until_pause: TWO,
            },
        );

        meta.update();
        assert_eq!(
            meta.direction(),
            Direction::ForwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.now(), 1);

        meta.update();
        assert_eq!(meta.direction(), Direction::Pause);
        assert_eq!(meta.now(), 1);
    }

    #[test]
    fn log_backward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            0,
            1,
            1,
            Direction::BackwardLog {
                updates_until_pause: TWO,
            },
        );

        meta.update();
        assert_eq!(
            meta.direction(),
            Direction::BackwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.now(), 0);

        meta.update();
        assert_eq!(meta.direction(), Direction::Pause);
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
        let mut meta = arrange(None, 1, 2, 3, Direction::Pause);

        assert_eq!(meta.queue_log(0), Err(1..3));
        assert_eq!(meta.queue_log(3), Err(1..3));
    }
}
