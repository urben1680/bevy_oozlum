use std::num::Wrapping;

use bevy::{ecs::query::WorldQuery, prelude::{Entity, Without, Component}};

pub mod controller;
pub mod commands;
pub mod system_mutation;
pub mod event;

pub const MAX_LOG_LEN: Ticks = Ticks::MAX;
pub const MAX_LOG_LEN_USIZE: usize = MAX_LOG_LEN as usize;

pub type Ticks = u16;
pub type Timestamp = Wrapping<Ticks>;

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