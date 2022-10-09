use std::{
    any::type_name,
    collections::VecDeque,
    fmt::Debug,
    marker::PhantomData,
    mem::{needs_drop, MaybeUninit},
    num::Wrapping,
};

use bevy::prelude::{Commands, Component};

use crate::{
    commands::ReversibleCommand, controller::Controller, Ticks, TicksRelative, LOG_LEN,
    MAX_LOG_INDEX,
};

use super::{NextTransition, StateOption};

#[derive(Component)]
pub struct Log<Marker, Transition, Index> {
    entry_index: usize,
    entries: VecDeque<LogEntry<Transition, Index>>,
    //last_update aber das hilft bei fast
    p: PhantomData<Marker>,
}

struct LogEntry<Transition, Index> {
    transition: MaybeUninit<Transition>, //is always init except it is the entry in the back of the log's `entries`.
    time_stamp: Wrapping<Ticks>,
    state_index: Index,
}

impl<Marker, Transition, Index: Copy + Debug> Log<Marker, Transition, Index> {
    pub(super) fn advance<'w, 's, State: StateOption<Index = Index>>(
        &mut self,
        states: &State::Param<'w>,
        controller: &Controller,
        commands: Commands<'w, 's>,
        mut advance: impl FnMut(&State::Output, Wrapping<Ticks>),
        mut next_transition: impl FnMut(
            &State::Output,
            Wrapping<Ticks>,
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
    ) {
        const FN: &'static str = "advance_system";
        let latest = self.entries.back().unwrap_or_else(|| {
            panic!(
                "`Log<{}>` at `{FN}`: `entries` should not be empty.",
                type_name::<Marker>()
            )
        });
        let state = State::get_state(states, latest.state_index).unwrap_or_else(|len|
            panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
            type_name::<Marker>(), latest.state_index));
        advance(state, controller.time_stamp());
        if let Some(next) = next_transition(state, controller.time_stamp()) {
            controller.send_commands(next.commands, commands);
            self.advance_next::<State>(
                next.next_state_index,
                next.transition,
                states,
                controller,
                state,
                advance_transition,
            );
        }
    }
    pub(super) fn advance_fast<'w, 's, State: StateOption<Index = Index>>(
        &mut self,
        states: &State::Param<'w>,
        controller: &Controller,
        commands: Commands<'w, 's>,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Option<Wrapping<Ticks>>,
        mut next_transition: impl FnMut(
            &State::Output,
            Wrapping<Ticks>,
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
        debug: impl Fn() -> String,
    ) {
        const FN: &'static str = "advance_fast_system";
        let latest = self.entries.back().unwrap_or_else(|| {
            panic!(
                "`Log<{}>` at `{FN}`: `entries` should not be empty.",
                type_name::<Marker>()
            )
        });
        let target = controller.target_time_stamp();
        if self.entries.len() > 1 {
            let second = &self.entries[self.entries.len() - 2];
            if !controller
                .time_stamp()
                .further_in_the_future(latest.time_stamp, second.time_stamp)
            {
                //Fast forward already happened because `latest` is in the future
                assert_ne!(controller.time_stamp(), target, "`Log<{}>` at `{FN}`: `advance_up_to_transition_or_limit` should not be skipped at the end of fast-forward.", type_name::<Marker>());
                return;
            }
        }
        let state = State::get_state(states, latest.state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
            type_name::<Marker>(), latest.state_index));
        if let Some(time_stamp) =
            advance_up_to_transition_or_limit(state, controller.time_stamp(), target)
        {
            let next = next_transition(state, target).unwrap_or_else(||panic!("`Log<{}>` at `{FN}`: `next_transition` should return `Some`. `advance_up_to_transition_or_limit` was called with state at index: {:?}, time stamp: `Wrapping({})` and target: `Wrapping({target})`. `next_transition` was called with the same state and `target` afterwards. Additional information:\n{}",
                type_name::<Marker>(), latest.state_index, controller.time_stamp(), debug()
            ));
            controller.send_delayed_commands(next.commands, time_stamp, commands);
            self.advance_next::<State>(
                next.next_state_index,
                next.transition,
                states,
                controller,
                state,
                advance_transition,
            );
        }
    }
    fn advance_next<'w, State: StateOption<Index = Index>>(
        &mut self,
        next_state_index: Index,
        transition: Transition,
        states: &State::Param<'w>,
        controller: &Controller,
        state: &State::Output,
        mut advance_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
    ) {
        const FN: &'static str = "advance_next";
        //capacity maximum is handled at systems calling `self.age_check_system`.
        //reserve as little as possible but remember the current capacity in saves to minimize future reservations.
        self.entries.reserve_exact(1);
        self.entry_index += 1;
        let next_state = State::get_state(states, next_state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {next_state_index:?} for the transition's future state, length is {len}.", 
            type_name::<Marker>()));
        advance_transition(state, next_state, &transition, controller.time_stamp());
        self.entries
            .back_mut()
            .unwrap_or_else(|| {
                panic!(
                    "`Log<{}>` at `{FN}`: `entries` should not be empty.",
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
    pub(super) fn advance_log<'w, State: StateOption<Index = Index>>(
        &mut self,
        states: &State::Param<'w>,
        controller: &Controller,
        mut advance: impl FnMut(&State::Output, Wrapping<Ticks>),
        mut advance_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
    ) {
        const FN: &'static str = "advance_log_system";
        let entry = self.entries.get(self.entry_index).unwrap_or_else(||panic!("`Log<{}>` at `{FN}`: `entries` should contain an element at index {}, length is {}.", 
            type_name::<Marker>(), self.entry_index, self. entries.len()));
        let state = State::get_state(states, entry.state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
            type_name::<Marker>(), entry.state_index));
        advance(state, controller.time_stamp());
        if let Some(next) = self.entries.get(self.entry_index + 1) {
            if controller.time_stamp() != next.time_stamp {
                return;
            }
            let next_state = State::get_state(states, next.state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?} for the transition's future state, length is {len}.", 
                type_name::<Marker>(), entry.state_index));
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            advance_transition(state, next_state, transition, controller.time_stamp());
            self.entry_index += 1;
        }
    }
    pub(super) fn advance_log_fast<
        'w,
        State: StateOption<Index = Index>,
        const FORCE_ADVANCE: bool,
    >(
        &mut self,
        states: &State::Param<'w>,
        controller: &Controller,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &State::Output,
            Wrapping<Ticks>,
            Wrapping<Ticks>,
        ) -> Option<Wrapping<Ticks>>,
        mut advance_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
    ) {
        const FN: &'static str = "advance_log_fast";
        /*
        It needs to be prevented that `advance_up_to_transition_or_limit` is called twice for an entry
        PROBLEM: woher current_time_stamp? Advance könnte für aktuellen entry bereits ausgeführt worden sein
        LÖSUNG: weiteres system: advance_log_fast_init, ggf revert_log_fast_init, derzeit über FORCE_ADVANCE
        PROBLEM: Zwischenmöglichkeit das Fast None zurückgibt funktionieren dann nicht mehr
        Ein Timestamp in log hilft nicht zuversichtlich da das system vielleicht danach nicht mehr lief und erst zB zwei ticks später anlief

        fast forward muss an bedingungen geknüpft werden:
        - darf nicht von der aktualität anderer systeme abhängig sein, außer sie sind zum zeitpunkt von transitionen aktuell
        - anderer systeme dürfen nicht von der aktualität abhängig sein, wenn dann nur indirekt über commands

        sonst:
        | second | back |
        |||||||||||||||||  // ticks (right end is target)
        | ^now   |         // wait until transition to call `advance_up_to_transition_or_limit` of `back`, assert time stamp with now
        ^now               // call `advance_up_to_transition_or_limit` of `second`, assert timestamp of next if present
        */
        let entry = self.entries.get(self.entry_index).unwrap_or_else(||panic!("`Log<{}>` at `{FN}`: `entries` should contain an element at index {}, length is {}.", 
            type_name::<Marker>(), self.entry_index, self. entries.len()));
        if entry.time_stamp != controller.time_stamp() {
            return;
        }
        if let Some(next) = self.entries.get(self.entry_index + 1) {
        } else {
        }
    }
    pub(super) fn revert_log<'w, State: StateOption<Index = Index>>(
        &mut self,
        states: &State::Param<'w>,
        controller: &Controller,
        mut revert_log: impl FnMut(&State::Output, Wrapping<Ticks>),
        mut revert_transition: impl FnMut(&State::Output, &State::Output, &Transition, Wrapping<Ticks>),
    ) {
        const FN: &'static str = "revert_log_system";
        let mut entry = self.entries.get(self.entry_index).unwrap_or_else(||panic!("`Log<{}>` at `{FN}`: `entries` should contain an element at index {}, length is {}.", 
            type_name::<Marker>(), self.entry_index, self. entries.len()));
        let mut state = State::get_state(states, entry.state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?}, length is {len}.", 
            type_name::<Marker>(), entry.state_index));
        if entry.time_stamp == controller.time_stamp() {
            self.entry_index -= 1;
            entry = self.entries.get(self.entry_index).unwrap_or_else(||panic!("`Log<{}>` at `{FN}`: `entries` should contain an element at index {} for the transition's past state, length is {}.", 
                type_name::<Marker>(), self.entry_index, self. entries.len()));
            let past_state = State::get_state(states, entry.state_index).unwrap_or_else(|len|panic!("`Log<{}>` at `{FN}`: `states` should contain an element at index {:?} for the transition's past state, length is {len}.", 
                type_name::<Marker>(), entry.state_index));
            let transition = unsafe {
                // SAFETY: transitions are always init if they are not stored in the back end of the log deque
                entry.transition.assume_init_ref()
            };
            revert_transition(past_state, state, transition, controller.time_stamp());
            state = past_state;
        }
        revert_log(state, controller.time_stamp());
    }
    //todo revert fast log
    pub(super) fn log_end(&mut self) {
        const FN: &'static str = "log_end";
        if needs_drop::<Transition>() {
            //the back end of the deque has no initialized transition
            //it needs to be made sure that after the truncation the new back end is also not/no longer initialized to uphold this rule
            assert!(
                self.entry_index < self.entries.len(),
                "`Log<{}>` at `{FN}`: `entry_index` {} should be smaller than length {}.",
                type_name::<Marker>(),
                self.entry_index,
                self.entries.len()
            );
            let range = self.entry_index..(self.entries.len() - 1);
            self.entries.range_mut(range).for_each(|entry| unsafe {
                // SAFETY: the given range makes sure only initialized transitions are dropped
                entry.transition.assume_init_drop();
            });
        }
        self.entries.truncate(self.entry_index + 1);
    }
    pub(super) fn age_check(&mut self, controller: &Controller) {
        const FN: &'static str = "age_check_system";
        let forget = controller.time_stamp() - Wrapping(MAX_LOG_INDEX);
        let second_time_stamp = self.entries.get(1).map(|second| second.time_stamp);
        let oldest = self.entries.front_mut().unwrap_or_else(|| {
            panic!(
                "`Log<{}>` at `{FN}`: `entries` should not be empty.",
                type_name::<Marker>()
            )
        });
        if second_time_stamp == Some(forget) {
            if needs_drop::<Transition>() {
                unsafe {
                    // SAFETY: entries[0].transition is always uninit, every other is init
                    oldest.transition.assume_init_drop();
                }
            }
            self.entries.pop_front();
        } else if oldest.time_stamp + Wrapping(1) == forget {
            oldest.time_stamp = forget;
        }
    }
}
