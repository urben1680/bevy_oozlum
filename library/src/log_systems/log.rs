use std::{
    any::type_name,
    collections::VecDeque,
    fmt::Debug,
    marker::PhantomData,
    mem::{needs_drop, MaybeUninit},
    num::Wrapping,
};

use bevy::prelude::{Commands, Component};

use crate::{controller::Controller, Ticks, TicksRelative, MAX_LOG_INDEX, LOG_LEN};

use super::{NextTransition, StateOption};

#[derive(Component)]
pub struct Log<Marker, Transition, Index> {
    entry_index: Ticks,
    entries: VecDeque<LogEntry<Transition, Index>>,
    p: PhantomData<Marker>, //make the log type unique so multiple log components are possible and panics are more helpful
}

struct LogEntry<Transition, Index> {
    transition: MaybeUninit<Transition>, //is always init except it is the entry in the back of the log's `entries`.
    time_stamp: Wrapping<Ticks>,
    state_index: Index,
}

impl<Marker, Transition, Index: Copy + Debug> Log<Marker, Transition, Index> {
    /// Log mutation to be called during `Progress::Advance`.
    ///
    /// Function arguments:
    /// - `advance`: current state, transitioned time stamp, current time stamp
    /// - `next_transition`: current state, transitioned time stamp, current time stamp, returning next transition
    /// - `advance_transition`: past state, future state, transition, current time stamp
    pub(super) fn advance<'w, 's, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        commands: &mut Commands<'_, '_>,
        mut advance: impl FnMut(&mut Params, &State::Output, Wrapping<Ticks>, Wrapping<Ticks>),
        mut next_transition: impl FnMut(
            &mut Params,
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
    ) {
        const FN: &'static str = "advance";
        let latest = self.entries.back().unwrap_or_else(|| {
            panic!(
                "`Log<{}>::{FN}`: `entries` should not be empty.",
                type_name::<Marker>()
            )
        });
        let state = State::get_state(states, latest.state_index).unwrap_or_else(|len| {
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.",
                type_name::<Marker>(), latest.state_index
            )
        });
        advance(params, state, latest.time_stamp, controller.time_stamp());
        if let Some(next) =
            next_transition(params, state, latest.time_stamp, controller.time_stamp())
        {
            controller.send_commands(next.commands, commands);
            self.advance_next::<State, Params>(
                params,
                next.next_state_index,
                next.transition,
                states,
                controller,
                state,
                advance_transition,
            );
        }
    }
    /// Log mutation to be called during `Progress::AdvanceFast`.
    ///
    /// Function arguments:
    /// - `advance_up_to_transition_or_limit`: current state, transitioned time stamp, current time stamp, limit time stamp,
    /// returning time stamp at transition or limit if it happens earlier
    /// - `next_transition`: current state, transitioned time stamp, current time stamp, returning next transition
    /// - `advance_transition`: past state, future state, transition, current time stamp
    pub(super) fn advance_fast<'w, 's, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        commands: &mut Commands<'_, '_>,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &mut Params,
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Wrapping<Ticks>,
        mut next_transition: impl FnMut(
            &mut Params,
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
        debug: impl Fn(&mut Params) -> String,
    ) {
        const FN: &'static str = "advance_fast";
        let latest = self.entries.back().unwrap_or_else(|| {
            panic!(
                "`Log<{}>::{FN}`: `entries` should not be empty.",
                type_name::<Marker>()
            )
        });
        let limit = controller.forward_fast_limit();
        if self.entries.len() > 1 {
            let second = &self.entries[self.entries.len() - 2];
            if !controller
                .time_stamp()
                .further_in_the_future(latest.time_stamp, second.time_stamp)
            {
                //Fast forward already happened because `latest` is in the future
                assert_ne!(
                    controller.time_stamp(), limit, "`Log<{}>::{FN}`: `advance_up_to_transition_or_limit` should not be skipped at the end of fast-forward.", 
                    type_name::<Marker>()
                );
                return;
            }
        }
        let state = State::get_state(states, latest.state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
                type_name::<Marker>(), latest.state_index
            );
        });
        let time_stamp = advance_up_to_transition_or_limit(
            params,
            state,
            latest.time_stamp,
            controller.time_stamp(),
            limit,
        );
        if let Some(next) = next_transition(params, state, latest.time_stamp, limit) {
            controller.send_delayed_commands(next.commands, time_stamp, commands);
            self.advance_next::<State, Params>(
                params,
                next.next_state_index,
                next.transition,
                states,
                controller,
                state,
                advance_transition,
            );
        } else if time_stamp != limit {
            panic!(
                "`Log<{}>::{FN}`: `next_transition` should return `Some`. `advance_up_to_transition_or_limit` was called with state at index: {:?}, time stamp: `Wrapping({})` and limit: `Wrapping({limit})` and did not return the limit, `next_transition` was called with the same state and `limit` afterwards. Additional information:\n{}",
                type_name::<Marker>(), latest.state_index, controller.time_stamp(), debug(params)
            );
        }
    }
    fn advance_next<'w, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        next_state_index: Index,
        transition: Transition,
        states: &State::Param<'w>,
        controller: &Controller,
        state: &State::Output,
        mut advance_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
    ) {
        const FN: &'static str = "advance_next";
        //capacity maximum is handled at systems calling `self.age_check_system`.
        //reserve as little as possible but remember the current capacity in saves to minimize future reservations.
        self.entries.reserve_exact(1);
        if self.entry_index == MAX_LOG_INDEX{
            assert_eq!(
                self.entries.len(), LOG_LEN,
                "`Log<{}>::{FN}`: `entries.len()` should return {MAX_LOG_INDEX} instead of {}.",
                type_name::<Marker>(), self.entries.len()
            );
            let oldest = self.entries.pop_front();
            if needs_drop::<Transition>(){
                unsafe{
                    //SAFETY: Only the other end of the deque contains an uninit transition
                    oldest.unwrap().transition.assume_init();
                }
            }
        } else {
            self.entry_index += 1;
        }
        let next_state = State::get_state(states, next_state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {next_state_index:?} for the transition's future state, length is {len}.", 
                type_name::<Marker>()
            )
        });
        advance_transition(
            params,
            state,
            next_state,
            &transition,
            controller.time_stamp(),
        );
        self.entries
            .back_mut()
            .unwrap_or_else(|| {
                panic!(
                    "`Log<{}>::{FN}`: `entries` should not be empty.",
                    type_name::<Marker>()
                )
            })
            .transition
            .write(transition);
        self.entries.push_back(LogEntry {
            transition: MaybeUninit::zeroed(),
            time_stamp: controller.time_stamp(),
            state_index: next_state_index,
        });
    }
    /// Log mutation to be called during `Progress::AdvanceLog`.
    ///
    /// Function arguments:
    /// - `advance`: current state, transitioned time stamp, current time stamp
    /// - `advance_transition`: past state, future state, transition, current time stamp
    pub(super) fn advance_log<'w, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        mut advance: impl FnMut(&mut Params, &State::Output, Wrapping<Ticks>, Wrapping<Ticks>),
        mut advance_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
    ) {
        const FN: &'static str = "advance_log";
        let entry_index = self.entry_index as usize;
        let entry = self.entries.get(entry_index).unwrap_or_else(||{
            panic!(
                "`Log<{}>::{FN}`: `entries` should contain an element at index {}, length is {}.", 
                type_name::<Marker>(), entry_index, self. entries.len()
            )
        });
        let state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
                type_name::<Marker>(), entry.state_index
            )
        });
        advance(params, state, entry.time_stamp, controller.time_stamp());
        if let Some(next) = self.entries.get(entry_index + 1) {
            if controller.time_stamp() != next.time_stamp {
                return;
            }
            let next_state = State::get_state(states, next.state_index).unwrap_or_else(|len|panic!("`Log<{}>::{FN}`: `states` should contain an element at index {:?} for the transition's future state, length is {len}.", 
                type_name::<Marker>(), entry.state_index));
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            advance_transition(
                params,
                state,
                next_state,
                transition,
                controller.time_stamp(),
            );
            self.entry_index += 1;
        }
    }
    /// Log mutation to be called during `Progress::AdvanceLogEnd`.
    ///
    /// Function arguments:
    /// - `advance_up_to_transition_or_limit`: current state, transitioned time stamp, current time stamp, limit time stamp,
    /// returning time stamp at transition or limit if it happens earlier
    /// - `advance_transition`: past state, future state, transition, current time stamp
    pub(super) fn advance_log_fast<'w, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &mut Params,
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Wrapping<Ticks>,
        mut advance_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
        debug: impl Fn(&mut Params) -> String,
    ) {
        const FN: &'static str = "advance_log_fast";
        let entry_index = self.entry_index as usize;
        let entry = self.entries.get(entry_index).unwrap_or_else(||{
            panic!(
                "`Log<{}>::{FN}`: `entries` should contain an element at index {}, length is {}.", 
                type_name::<Marker>(), self.entry_index, self. entries.len()
            )
        });
        if !controller.fast_init() && entry.time_stamp != controller.time_stamp() {
            return;
        }
        let state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
                type_name::<Marker>(), entry.state_index
            )
        });
        let limit = controller.forward_fast_limit();
        let time_stamp = advance_up_to_transition_or_limit(
            params,
            state,
            entry.time_stamp,
            controller.time_stamp(),
            limit,
        );
        if let Some(next) = self.entries.get(entry_index + 1) {
            assert_eq!(
                time_stamp, next.time_stamp,
                "`Log<{}>::{FN}` with init `{}`: `advance_up_to_transition_or_limit` should return the limit time stamp `Wrapping({})`, not `Wrapping({})`. `advance_up_to_transition_or_limit` was called with state index {:?} and time stamp `Wrapping({})`. Additional information:\n{}",
                type_name::<Marker>(), controller.fast_init(), limit, time_stamp, entry.state_index, controller.time_stamp(), debug(params)
            );
            self.entry_index += 1;
            let next_state = State::get_state(states, next.state_index).unwrap_or_else(|len|{
                panic!(
                    "`Log<{}>::{FN}`: `states` should contain an element at index {:?} for the transition's future state, length is {len}.", 
                    type_name::<Marker>(), next.state_index
                )
            });
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            advance_transition(params, state, next_state, transition, time_stamp);
        } else if time_stamp != limit {
            panic!(
                "`Log<{}>::{FN}`: `next_transition` should return `Some`. `advance_up_to_transition_or_limit` was called with state at index: {:?}, time stamp: `Wrapping({})` and limit: `Wrapping({limit})` and did not return the limit, `next_transition` was called with the same state and `limit` afterwards. Additional information:\n{}",
                type_name::<Marker>(), entry.state_index, controller.time_stamp(), debug(params)
            );
        }
    }
    /// Log mutation to be called during `Progress::RevertLog`.
    ///
    /// Function arguments:
    /// - `revert_log`: current state, transitioned time stamp, current time stamp
    /// - `revert_transition`: past state, future state, transition, current time stamp
    pub(super) fn revert_log<'w, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        mut revert: impl FnMut(&mut Params, &State::Output, Wrapping<Ticks>, Wrapping<Ticks>),
        mut revert_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
    ) {
        const FN: &'static str = "revert_log";
        let mut entry = self.entries.get(self.entry_index as usize).unwrap_or_else(||{
            panic!(
                "`Log<{}>::{FN}`: `entries` should contain an element at index {}, length is {}.", 
                type_name::<Marker>(), self.entry_index, self. entries.len()
            )
        });
        let mut state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
                type_name::<Marker>(), entry.state_index
            )
        });
        if entry.time_stamp == controller.time_stamp() && self.entry_index != 0 {
            self.entry_index -= 1;
            entry = &self.entries[self.entry_index as usize];
            let past_state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
                panic!(
                    "`Log<{}>::{FN}`: `states` should contain an element at index {:?} for the transition's past state, length is {len}.", 
                    type_name::<Marker>(), entry.state_index
                )
            });
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            revert_transition(
                params,
                past_state,
                state,
                transition,
                controller.time_stamp(),
            );
            state = past_state;
        }
        revert(params, state, entry.time_stamp, controller.time_stamp());
    }
    /// Log mutation to be called during `Progress::RevertLogEnd`.
    ///
    /// Function arguments:
    /// - `revert_down_to_transition_or_limit`: current state, transitioned time stamp, current time stamp
    /// - `revert_transition`: past state, future state, transition, current time stamp
    pub(super) fn revert_log_fast<'w, State: StateOption<Index = Index>, Params>(
        &mut self,
        params: &mut Params,
        states: &State::Param<'w>,
        controller: &Controller,
        mut revert_down_to_transition_or_limit: impl FnMut(
            &mut Params,
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ),
        mut revert_transition: impl FnMut(
            &mut Params,
            &State::Output,
            &State::Output,
            &Transition,
            Wrapping<Ticks>,
        ),
    ) {
        const FN: &'static str = "revert_log_fast";
        if !controller.fast_init() && self.entry_index == 0 {
            return;
        }
        let mut entry = self.entries.get(self.entry_index as usize).unwrap_or_else(||{
            panic!(
                "`Log<{}>::{FN}`: `entries` should contain an element at index {}, length is {}.", 
                type_name::<Marker>(), self.entry_index, self. entries.len()
            )
        });
        if !controller.fast_init() && entry.time_stamp != controller.time_stamp() {
            return;
        }
        let mut transitioned = entry.time_stamp;
        let mut state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
            panic!(
                "`Log<{}>::{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
                type_name::<Marker>(), entry.state_index
            )
        });
        if self.entry_index != 0 {
            self.entry_index -= 1;
            entry = &self.entries[self.entry_index as usize];
            transitioned = entry.time_stamp;
            let past_state = State::get_state(states, entry.state_index).unwrap_or_else(|len|{
                panic!(
                    "`Log<{}>::{FN}`: `states` should contain an element at index {:?} for the transition's past state, length is {len}.", 
                    type_name::<Marker>(), entry.state_index
                )
            });
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            revert_transition(
                params,
                past_state,
                state,
                transition,
                controller.time_stamp(),
            );
            state = past_state;
        }
        if self.entry_index == 0 {
            transitioned = controller.log_start();
        }
        revert_down_to_transition_or_limit(params, state, transitioned, controller.time_stamp());
    }
    /// Log mutation to be called during log end.
    pub(super) fn log_end(&mut self) {
        const FN: &'static str = "log_end";
        let entry_index = self.entry_index as usize;
        if needs_drop::<Transition>() {
            //the back end of the deque has no initialized transition
            //it needs to be made sure that after the truncation the new back end is also not/no longer initialized to uphold this rule
            assert!(
                entry_index < self.entries.len(),
                "`Log<{}>::{FN}`: `entry_index` {} should be smaller than length {}.",
                type_name::<Marker>(),
                self.entry_index,
                self.entries.len()
            );
            let range = entry_index..(self.entries.len() - 1);
            self.entries.range_mut(range).for_each(|entry| unsafe {
                // SAFETY: the given range makes sure only initialized transitions are dropped
                entry.transition.assume_init_drop();
            });
        }
        self.entries.truncate(entry_index + 1);
    }
    /// Log mutation to be called at every non-pause progress.
    pub(super) fn age_check(&mut self, controller: &Controller) {
        const FN: &'static str = "age_check";
        let second_time_stamp = self.entries.get(1).map(|second| second.time_stamp);
        if second_time_stamp == Some(controller.forget()) {
            if needs_drop::<Transition>() {
                let oldest = self.entries.front_mut().unwrap_or_else(|| {
                    panic!(
                        "`Log<{}>::{FN}`: `entries` should not be empty.",
                        type_name::<Marker>()
                    )
                });
                unsafe {
                    // SAFETY: entries[0].transition is always uninit, every other is init
                    oldest.transition.assume_init_drop();
                }
            }
            self.entries.pop_front();
        }
    }
}
