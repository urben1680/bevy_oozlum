use std::{num::Wrapping, any::type_name};

use bevy::{ecs::system::{SystemParam, Resource}, prelude::Res};

use crate::{commands::CommandsScope, controller::Controller, Ticks, MAX_LOG_LEN};

use super::{log::{LogEntryTrait, Log}, next_transition::NextTransitionTrait};

pub(super) const ID_ADVANCE: usize = 0;
pub(super) const ID_ADVANCE_TIMESTAMP: usize = ID_ADVANCE + 1;
pub(super) const ID_ADVANCE_LOG: usize = ID_ADVANCE_TIMESTAMP + 1;
pub(super) const ID_ADVANCE_LOG_END: usize = ID_ADVANCE_LOG + 1;
pub(super) const ID_REVERT_LOG: usize = ID_ADVANCE_LOG_END + 1;
pub(super) const ID_REVERT_LOG_END: usize = ID_REVERT_LOG + 1;
pub(super) const ID_LOG_END: usize = ID_REVERT_LOG_END + 1;
pub(super) const ID_LOG_AGE_CHECK: usize = ID_LOG_END + 1;

pub(super) trait GetState: SystemParam{
    type Output;
    fn get_state(&self, index: usize) -> &Self::Output;
}

impl<'w, T: Resource> GetState for Res<'w, Vec<T>>{
    type Output = T;
    fn get_state(&self, index: usize) -> &Self::Output{
        match self.get(index){
            Some(state) => state,
            None => panic!("Could not find state in `Vec<{}>` at index {index}, vector length is {}.", type_name::<T>(), self.len())
        }
    }
}

impl GetState for (){
    type Output = ();
    fn get_state(&self, _index: usize) -> &Self::Output{
        &()
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_system<
    'w, 's,
    Params,
    States: GetState,
    CommandsType: CommandsScope<'w, 's>,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    params: &mut Params, 
    states: &States,
    controller: &Controller,
    commands: CommandsType,
    log: &mut Log<Entry, Transition, Marker>,
    next_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output
    ) -> Option<NextTransition>,
    advance: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output
    ),
    advance_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output,
        &<States as GetState>::Output,
        &Transition
    )
){
    let entry = match log.entries.back(){
        Some(entry) => entry,
        None => panic!("Log for {} should not be empty to get entry at back end.", type_name::<Marker>())
    };
    let state = states.get_state(entry.state_index());
    advance(params, controller.time_stamp(), state);
    if let Some(next) = next_transition(params, controller.time_stamp(), state){
        advance_next(None, next, params, states, controller, commands, log, state, advance_transition);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn advance_next<
    'w, 's,
    Params,
    States: GetState,
    CommandsType: CommandsScope<'w, 's>,
    Entry: LogEntryTrait<Transition>,
    NextTransition: NextTransitionTrait<Transition, Marker>,
    Transition: Resource,
    Marker: Resource
>(
    delay_commands_to: Option<Wrapping<Ticks>>,
    next: NextTransition,
    params: &mut Params,
    states: &States,
    controller: &Controller,
    commands: CommandsType,
    log: &mut Log<Entry, Transition, Marker>,
    state: &<States as GetState>::Output,
    advance_transition: fn(
        &mut Params,
        Wrapping<Ticks>,
        &<States as GetState>::Output,
        &<States as GetState>::Output,
        &Transition
    )
){
    let forget = if log.entries.len() < MAX_LOG_LEN.min(log.entries.capacity()){
        log.entry_index += 1;
        None
    } else {
        log.entries.pop_front();
        let age = log.entries.front().unwrap().time_stamp();
        Some(age - Wrapping(1))
    };
    let (next_state_index, transition, command) = next.explode();
    let next_state = states.get_state(next_state_index);
    advance_transition(params, controller.time_stamp(), state, next_state, &transition);
    log.entries.back_mut().unwrap().set_transition(transition);
    log.entries.push_back(Entry::new(controller.time_stamp(), next_state_index));
    match (forget, command){
        (None, None) => {},
        (Some(forget), None) => {
            commands.get_command_scope(|mut commands|{
                forget.send(&mut commands);
            });
        },
        (forget, Some(command)) => {
            commands.get_command_scope(|mut commands|{
                if let Some(forget) = forget{
                    forget.send(&mut commands);
                }
                if let Some(target) = delay_commands_to{
                    //ReversibleCommands::delayed(commands, command, target);
                } else {
                    todo!();
                    //command(ReversibleCommands::new(&mut commands));
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These constants are used as genric constants, it would be a bug if two or more of them had the same value.
    #[test]
    fn system_ids_constants_are_unique() {
        let mut ids = vec![
            ID_ADVANCE, ID_ADVANCE_TIMESTAMP, ID_ADVANCE_LOG, ID_ADVANCE_LOG_END, 
            ID_REVERT_LOG, ID_REVERT_LOG_END, ID_LOG_END, ID_LOG_AGE_CHECK
        ];
        let len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), len, "System id constants contain duplicate values.");
    }
}