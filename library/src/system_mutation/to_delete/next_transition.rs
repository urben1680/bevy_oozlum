use crate::commands::{NextCommands, ReversibleCommands};

pub struct NextTransitionWithState<Transition, Marker: Send + Sync + 'static> {
    pub(super) next_state_index: usize,
    pub(super) transition: Transition,
    pub(super) commands: Option<NextCommands<Marker>>,
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionWithState<Transition, Marker> {
    pub fn new(next_state_index: usize, transition: Transition) -> Self {
        Self {
            next_state_index,
            transition,
            commands: None,
        }
    }
    pub fn new_with_commands<F: FnOnce(ReversibleCommands<Marker>) + Send + Sync + 'static>(
        next_state_index: usize,
        transition: Transition,
        commands: F,
    ) -> Self {
        Self {
            next_state_index,
            transition,
            commands: Some(Box::new(commands)),
        }
    }
}

pub struct NextTransition<Transition, Marker: Send + Sync + 'static> {
    pub(super) transition: Transition,
    pub(super) commands: Option<NextCommands<Marker>>,
}

impl<Transition, Marker: Send + Sync + 'static> NextTransition<Transition, Marker> {
    pub fn new(transition: Transition) -> Self {
        Self {
            transition,
            commands: None,
        }
    }
    pub fn new_with_commands<F: FnOnce(ReversibleCommands<Marker>) + Send + Sync + 'static>(
        transition: Transition,
        commands: F,
    ) -> Self {
        Self {
            transition,
            commands: Some(Box::new(commands)),
        }
    }
}

pub(super) trait NextTransitionTrait<Transition, Marker: Send + Sync + 'static> {
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>);
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionTrait<Transition, Marker>
    for NextTransition<Transition, Marker>
{
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>) {
        (Default::default(), self.transition, self.commands)
    }
}

impl<Transition, Marker: Send + Sync + 'static> NextTransitionTrait<Transition, Marker>
    for NextTransitionWithState<Transition, Marker>
{
    fn explode(self) -> (usize, Transition, Option<NextCommands<Marker>>) {
        (self.next_state_index, self.transition, self.commands)
    }
}
