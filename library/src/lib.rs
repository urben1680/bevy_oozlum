use bevy::{ecs::query::WorldQuery, prelude::{Entity, Without, Component}};

pub mod controller;
pub mod commands;
pub mod system_mutation;
pub mod event;

pub const MAX_LOG_INDEX: Ticks = Ticks::MAX;
pub const MAX_LOG_INDEX_USIZE: usize = MAX_LOG_INDEX as usize;
pub const MAX_LOG_LEN: usize = MAX_LOG_INDEX_USIZE + 1;
pub const DEFAULT_TIME_STEP: f64 = 0.02;
pub const FORGET_SYNC_SENDER_CAPACITY: usize = 1024;
pub const DELAYED_COMMANDS_TICKS_CAPACITY: usize = Ticks::MAX as usize >> 1; //jumping from morning to evening
pub const DELAYED_COMMANDS_SYNC_SENDER_CAPACITY: usize = 1024;

/// Type that stores the ticks systems work by.
/// MAX value is also the limit how many ticks can be logged.
/// Timestamps are stored as `std::num::Wrapping<Ticks>`.
pub type Ticks = u16;

/// Component that should be always queried in `Query`s (instead of `Entity`).
#[derive(WorldQuery)]
pub struct PresentEntity{
    pub entity: Entity,
    filter: Without<DespawnedEntity>
}

/// Buffer component/resource that contains despawned data so it can be recovered.
pub struct Despawned<T>(pub T);

impl<T: Component> Component for Despawned<T>{
    type Storage = <T as Component>::Storage;
}

/// Flag Component to mark an `Entity` as despawned.
#[derive(Component)]
pub struct DespawnedEntity;

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert!(true);
    }
}
*/