use std::{collections::VecDeque, fmt::Debug, marker::PhantomData, num::Wrapping};

use bevy::prelude::{Commands, Component, Resource};

use crate::{controller::Controller, Ticks, TicksRelative, ToTimeStamp};

use super::{NextTransition, StateOption};

#[derive(Component, Resource)]
pub struct Log<Marker, Transition, Index> {
    log_index: Ticks,
    log: VecDeque<LogTransition<Transition, Index>>,
    pre_log: Meta<Index>,
    p: PhantomData<Marker>, //make the log type unique so multiple log components are possible and panics are more helpful
}

struct LogTransition<Transition, Index> {
    transition_with_previous: Transition,
    meta: Meta<Index>,
}

#[derive(Clone, Copy)]
struct Meta<Index> {
    transitioned: Wrapping<Ticks>,
    state_index: Index,
}

pub(crate) const fn assert_max_log_index(max_log_index: Ticks) {
    if max_log_index == Ticks::MAX {
        panic!("`max_log_index` should not be equal `Ticks::MAX` because log indices are off by one to adress pre_log meta");
    }
}

impl<Marker, Transition, Index: Copy + Debug> Log<Marker, Transition, Index> {
    const LOG_INDEX_OUT_OF_RANGE: &'static str =
        "`log_index` should be smaller or equal `log.len()`";
    fn latest(&self) -> &Meta<Index> {
        self.log
            .back()
            .map(|latest| &latest.meta)
            .unwrap_or(&self.pre_log)
    }
    fn before_latest(&self) -> Option<&Meta<Index>> {
        match self.log.len() {
            0 => None,
            1 => Some(&self.pre_log),
            len => Some(&self.log.get(len - 2).unwrap().meta),
        }
    }
    fn entry(&self) -> &Meta<Index> {
        if self.log_index == 0 {
            return &self.pre_log;
        }
        &self
            .log
            .get((self.log_index - 1) as usize)
            .expect(Self::LOG_INDEX_OUT_OF_RANGE)
            .meta
    }
    fn entry_with_transition(&self) -> (&Meta<Index>, Option<&Transition>) {
        if self.log_index == 0 {
            return (&self.pre_log, None);
        }
        self.log
            .get((self.log_index - 1) as usize)
            .map(|x| (&x.meta, Some(&x.transition_with_previous)))
            .expect(Self::LOG_INDEX_OUT_OF_RANGE)
    }
    fn before_entry(&self) -> Option<&Meta<Index>> {
        match self.log_index {
            0 => None,
            1 => Some(&self.pre_log),
            i => Some(
                &self
                    .log
                    .get((i - 2) as usize)
                    .expect(Self::LOG_INDEX_OUT_OF_RANGE)
                    .meta,
            ),
        }
    }
    fn after_entry(&self) -> Option<&LogTransition<Transition, Index>> {
        if self.log.len() == self.log_index as usize {
            if self.log.is_empty() && self.log_index != 0 {
                panic!("{}", Self::LOG_INDEX_OUT_OF_RANGE);
            }
            return None;
        }
        Some(
            &self
                .log
                .get(self.log_index as usize)
                .expect(Self::LOG_INDEX_OUT_OF_RANGE),
        )
    }
    pub(super) fn forward<State: StateOption<Index = Index>, RefParam, MutParam>(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        commands: &mut Commands<'_, '_>,
        mut advance: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
        ),
        mut next_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //past state
            &State::Output,  //future state
            &Transition,     //transition
            Wrapping<Ticks>, //current time stamp
        ),
    ) {
        let latest = self.latest();
        let state = State::get_state(states, latest.state_index);
        advance(
            ref_param,
            mut_param,
            state,
            latest.transitioned,
            controller.time_stamp(),
        );
        if let Some(next) = next_transition(
            ref_param,
            mut_param,
            state,
            latest.transitioned,
            controller.time_stamp(),
        ) {
            controller.send_commands(next.commands, commands, 0);
            self.next_transition::<State, RefParam, MutParam>(
                ref_param,
                mut_param,
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
    pub(super) fn forward_to<State: StateOption<Index = Index>, RefParam, MutParam>(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        commands: &mut Commands<'_, '_>,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //curent time stamp
            ToTimeStamp,     //limit time stamp
        ) -> Wrapping<Ticks>,
        mut next_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
        ) -> Option<NextTransition<State, Transition>>,
        advance_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
        debug: impl Fn(&RefParam, &mut MutParam) -> String,
    ) {
        let latest = self.latest();
        let limit = controller.to_time_stamp();
        if let Some(previous) = self.before_latest() {
            if latest
                .transitioned
                .further_in_the_future(controller.time_stamp(), previous.transitioned)
            {
                //transition already happened, wait with further mutations until latest is not in the future anymore
                //todo: why was here an error check? assert_ne!(controller.time_stamp(), latest.transitioned)
                return;
            }
        }
        let state = State::get_state(states, latest.state_index);
        let time_stamp = advance_up_to_transition_or_limit(
            ref_param,
            mut_param,
            state,
            latest.transitioned,
            controller.time_stamp(),
            limit,
        );
        if let Some(next) = next_transition(
            ref_param,
            mut_param,
            state,
            latest.transitioned,
            controller.time_stamp(),
        ) {
            controller.send_commands(next.commands, commands, limit.delta_abs - 1);
            self.next_transition::<State, RefParam, MutParam>(
                ref_param,
                mut_param,
                next.next_state_index,
                next.transition,
                states,
                controller,
                state,
                advance_transition,
            );
        } else if time_stamp != limit.to_time_stamp {
            panic!("todo");
        }
    }
    fn next_transition<State: StateOption<Index = Index>, RefParam, MutParam>(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        next_state_index: Index,
        transition: Transition,
        states: &State::Param<'_>,
        controller: &Controller,
        state: &State::Output,
        mut advance_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
    ) {
        if self.log_index == controller.consts().max_log_index {
            assert_eq!(self.log.len(), controller.consts().log_capacity, "todo");
            let oldest = self.log.pop_front().expect("todo");
            self.pre_log = oldest.meta;
        } else {
            //capacity maximum is handled at systems calling `self.age_check_system`.
            //reserve as little as possible but remember the current capacity in saves to minimize future reservations.
            self.log.reserve_exact(1);
            self.log_index += 1;
        }
        let next_state = State::get_state(states, next_state_index);
        advance_transition(
            ref_param,
            mut_param,
            state,
            next_state,
            &transition,
            controller.time_stamp(),
        );
        self.log.push_back(LogTransition {
            transition_with_previous: transition,
            meta: Meta {
                transitioned: controller.time_stamp(),
                state_index: next_state_index,
            },
        });
    }
    pub(super) fn forward_log<State: StateOption<Index = Index>, RefParam, MutParam>(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        mut advance: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
        ),
        mut advance_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
    ) {
        let entry = self.entry();
        let state = State::get_state(states, entry.state_index);
        advance(
            ref_param,
            mut_param,
            state,
            entry.transitioned,
            controller.time_stamp(),
        );
        if let Some(next) = self.after_entry() {
            if controller.time_stamp() != next.meta.transitioned {
                return;
            }
            let next_state = State::get_state(states, next.meta.state_index);
            advance_transition(
                ref_param,
                mut_param,
                state,
                next_state,
                &next.transition_with_previous,
                controller.time_stamp(),
            );
            self.log_index += 1;
        }
    }
    pub(super) fn forward_log_to<
        State: StateOption<Index = Index>,
        RefParam,
        MutParam,
        const INIT: bool,
    >(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        mut advance_up_to_transition_or_limit: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
            ToTimeStamp,     //limit time stamp
        ) -> Wrapping<Ticks>,
        mut advance_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
        debug: impl Fn(&RefParam, &mut MutParam) -> String,
    ) {
        let entry = self.entry();
        if !INIT && entry.transitioned != controller.time_stamp() {
            return;
        }
        let state = State::get_state(states, entry.state_index);
        let time_stamp = advance_up_to_transition_or_limit(
            ref_param,
            mut_param,
            state,
            entry.transitioned,
            controller.time_stamp(),
            controller.to_time_stamp(),
        );
        if let Some(next) = self.after_entry() {
            assert_eq!(time_stamp, next.meta.transitioned, "todo");
            let next_state = State::get_state(states, next.meta.state_index);
            advance_transition(
                ref_param,
                mut_param,
                state,
                next_state,
                &next.transition_with_previous,
                time_stamp,
            );
            self.log_index += 1;
        } else if time_stamp != controller.to_time_stamp().to_time_stamp {
            panic!("todo");
        }
    }
    pub(super) fn backward_log<State: StateOption<Index = Index>, RefParam, MutParam>(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        mut revert: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
        ),
        mut revert_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
    ) {
        let mut entry = self.entry_with_transition();
        let mut state = State::get_state(states, entry.0.state_index);
        let mut lower_index = false;
        if entry.0.transitioned == controller.time_stamp() {
            if let Some(previous) = self.before_entry() {
                let past_state = State::get_state(states, previous.state_index);
                revert_transition(
                    ref_param,
                    mut_param,
                    past_state,
                    state,
                    entry.1.expect("todo"),
                    controller.time_stamp(),
                );
                state = past_state;
                entry.0 = previous;
                lower_index = true;
            }
        }
        revert(
            ref_param,
            mut_param,
            state,
            entry.0.transitioned,
            controller.time_stamp(),
        );
        if lower_index {
            self.log_index -= 1;
        }
    }
    pub(super) fn backward_log_to<
        State: StateOption<Index = Index>,
        RefParam,
        MutParam,
        const INIT: bool,
    >(
        &mut self,
        ref_param: &RefParam,
        mut_param: &mut MutParam,
        states: &State::Param<'_>,
        controller: &Controller,
        mut revert_down_to_transition_or_limit: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output,  //current state
            Wrapping<Ticks>, //transitioned time stamp
            Wrapping<Ticks>, //current time stamp
            ToTimeStamp,     //limit time stamp
        ),
        mut revert_transition: impl FnMut(
            &RefParam,
            &mut MutParam,
            &State::Output, //past state
            &State::Output, //future state
            &Transition,
            Wrapping<Ticks>, //current time stamp
        ),
    ) {
        if !INIT && self.log_index == 0 {
            return;
        }
        let mut entry = self.entry_with_transition();
        if !INIT && entry.0.transitioned != controller.time_stamp() {
            return;
        }
        let mut state = State::get_state(states, entry.0.state_index);
        let mut lower_index = false;
        if let Some(previous) = self.before_entry() {
            let past_state = State::get_state(states, previous.state_index);
            revert_transition(
                ref_param,
                mut_param,
                past_state,
                state,
                entry.1.expect("todo"),
                controller.time_stamp(),
            );
            state = past_state;
            entry.0 = previous;
            lower_index = true;
        }
        revert_down_to_transition_or_limit(
            ref_param,
            mut_param,
            state,
            entry.0.transitioned,
            controller.time_stamp(),
            controller.to_time_stamp(),
        );
        if lower_index {
            self.log_index -= 1;
        }
    }
    /// Log mutation to be called during log end.
    pub(super) fn log_close(&mut self) {
        self.log.truncate(self.log_index as usize);
    }
    /// Log mutation to be called at every non-pause progress.
    pub(super) fn age_check(&mut self, controller: &Controller) {
        if let Some(second_oldest) = self.log.front() {
            if second_oldest.meta.transitioned == controller.forget() {
                self.pre_log = second_oldest.meta;
                self.log.pop_front();
                self.log_index -= 1;
            }
        }
    }
}
