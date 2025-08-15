use std::{any::TypeId, panic::Location};

use bevy::{
    ecs::{
        archetype::ArchetypeId,
        bundle::{Bundle, BundleId, InsertMode},
        change_detection::MaybeLocation,
        component::ComponentId,
        entity::{Entity, EntityCloner},
        resource::Resource,
        world::{EntityWorldMut, World},
    },
    platform::collections::{HashMap, HashSet},
};

use crate::meta::{NonLogNow, RevDirection};

use super::{
    BuffersUndoRedo, EntityRevDespawnedError, ResourceSwap, RevDespawnCleaner, RevDespawned,
    UndoRedo,
};

// todo: move to entity_world module except RevOpInProgress + friends
// does that work out with MaybeLocation of rev commands?
// alternativeley, move all logic to commands with correct location passing and make RevEntityWorldMut generate and apply them
// make all RevEntityWorldMut methods check for rev_despawned

#[cfg(test)]
mod test {
    use super::*;
    //todo
}
