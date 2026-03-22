use core::{any::type_name, marker::PhantomData};

use bevy_ecs::{
    change_detection::MaybeLocation,
    entity::{Entity, EntityHashSet},
    relationship::{
        Relationship, RelationshipAccessor, RelationshipSourceCollection, RelationshipTarget,
    },
    world::{EntityRef, EntityWorldMut, World, error::EntityMutableFetchError},
};
use bevy_log::error;

use crate::{
    prelude::UndoRedo,
    undo_redo::{IsRevDespawned, LOCATION_PREFIX, undo_redo_str},
};

#[cfg(test)]
mod test;

/// Compile-time assertion that [`Relationship`] and its [`RelationshipTarget`] do not contain extra
/// Non-ZST fields.
///
/// This limitation is needed because doing backups in `UndoRedo` buffers is too complex.
///
/// The associated constant needs to be assigned to a local variable with `let` to
/// utilize this assertion.
///
/// # Does compile
///
/// ```
/// # use bevy_oozlum::prelude::*;
/// # use bevy::prelude::*;
/// #[derive(Component)]
/// #[relationship(relationship_target = SlimChildren)]
/// pub struct SlimChildOf(pub Entity);
/// # #[derive(Component)]
/// # #[relationship_target(relationship = SlimChildOf)]
/// # pub struct SlimChildren(Vec<Entity>);
/// # App::new().add_systems(Update, system).init_resource::<RevMeta>().update();
///
/// fn system(meta: Res<RevMeta>, mut commands: Commands) {
///     if let Some(meta_past_len) = meta.get_meta_past_len() {
///         commands.spawn_empty().rev_detach_all_related::<SlimChildOf>(meta_past_len);
///     }
/// }
/// ```
///
/// # Does not compile
///
/// ```compile_fail
/// # use bevy_oozlum::prelude::*;
/// # use bevy::prelude::*;
/// #[derive(Component)]
/// #[relationship(relationship_target = FatChildren)]
/// pub struct FatChildOf {
///     #[relationship]
///     pub parent: Entity,
///     internal: u8, // non-ZST extra field
/// }
/// # #[derive(Component)]
/// # #[relationship_target(relationship = FatChildOf)]
/// # pub struct FatChildren(Vec<Entity>);
/// # App::new().add_systems(Update, system).init_resource::<RevMeta>().update();
///
/// fn system(meta: Res<RevMeta>, mut commands: Commands) {
///     if let Some(meta_past_len) = meta.get_meta_past_len() {
///         commands.spawn_empty().rev_detach_all_related::<FatChildOf>(meta_past_len);
///     }
/// }
/// ```
pub(super) trait SlimRelationship: Relationship {
    // todo: replace constant with actual trait bound when const generic features become available
    const ASSERT: () = {
        if size_of::<Self>() != size_of::<Entity>()
            || size_of::<Self::RelationshipTarget>()
                != size_of::<<Self::RelationshipTarget as RelationshipTarget>::Collection>()
        {
            // todo: add type name to panic message when that formatting becomes const
            panic!(
                "rev_* methods that handle relationships are not supported when extra data in any \
                of the two types is stored. Note that this limitiation is also present for rev_* \
                methods for component insertion/removal even though this cannot always be detected."
            )
        }
    };
}

impl<R: Relationship> SlimRelationship for R {}

/// Adds children of `parent` to `entities_set`.
pub(super) fn add_children(
    world: &World,
    parent: EntityRef,
    entities_set: &mut EntityHashSet,
    include_unlinked_related: bool,
) {
    if parent.is_rev_despawned() {
        return;
    }
    let children = parent
        .archetype()
        .components()
        .iter()
        .flat_map(|&component_id| {
            world
                .components()
                .get_info(component_id)
                .unwrap()
                .relationship_accessor()
                .and_then(|relationship| match *relationship {
                    RelationshipAccessor::RelationshipTarget { iter, linked_spawn }
                        if include_unlinked_related || linked_spawn =>
                    {
                        let ptr = parent.get_by_id(component_id).unwrap();
                        unsafe {
                            // SAFETY: given ComponentId matches the RelationshipAccessor of the component type
                            Some(iter(ptr))
                        }
                    }
                    _ => None,
                })
                .into_iter()
                .flatten()
        });

    for child in children {
        if !entities_set.insert(child) {
            continue;
        }
        let Ok(child_ref) = world.get_entity(child) else {
            entities_set.remove(child);
            continue;
        };
        if child_ref.is_rev_despawned() {
            entities_set.remove(child);
            continue;
        }
        add_children(world, child_ref, entities_set, include_unlinked_related);
    }
}

pub(super) struct AddRemoveRelated<
    R: Relationship,
    E: AsRef<[Entity]> + Send + 'static,
    const ADD: bool,
> {
    entity: Entity,
    related: E,
    caller: MaybeLocation,
    _p: PhantomData<R>,
}

impl<R: Relationship, E: AsRef<[Entity]> + Send + 'static, const ADD: bool>
    AddRemoveRelated<R, E, ADD>
{
    pub(super) fn new(entity: Entity, related: E, caller: MaybeLocation) -> Self {
        let _ = R::ASSERT;
        Self {
            entity,
            related,
            caller,
            _p: PhantomData,
        }
    }
    fn toggle<const UNDO: bool>(&mut self, world: &mut World) {
        let _ = R::ASSERT;
        match world.get_entity_mut(self.entity) {
            Ok(mut entity) => {
                if ADD ^ UNDO {
                    entity.add_related::<R>(self.related.as_ref());
                } else {
                    entity.remove_related::<R>(self.related.as_ref());
                }
            }
            Err(EntityMutableFetchError::NotSpawned(err)) => {
                error!(
                    "{} reversible relationship of {}{LOCATION_PREFIX}{} failed, {err}",
                    undo_redo_str::<UNDO>(),
                    type_name::<R>(),
                    self.caller
                );
            }
            Err(EntityMutableFetchError::AliasedMutability(_)) => unreachable!(),
        }
    }
}

impl<R: Relationship, E: AsRef<[Entity]> + Send + 'static, const ADD: bool> UndoRedo
    for AddRemoveRelated<R, E, ADD>
{
    fn undo(&mut self, world: &mut World) {
        self.toggle::<true>(world);
    }
    fn redo(&mut self, world: &mut World) {
        self.toggle::<false>(world);
    }
}

/// Calls `c` and returns new children of `entity` from that.
pub(super) fn get_new_related_entities<R: Relationship>(
    entity: &mut EntityWorldMut,
    c: impl for<'a, 'w> FnOnce(&'a mut EntityWorldMut<'w>) -> &'a mut EntityWorldMut<'w>,
) -> Vec<Entity> {
    match entity.get::<R::RelationshipTarget>() {
        Some(target) => {
            let existing_children: EntityHashSet = target.collection().iter().collect();
            c(entity)
                .get::<R::RelationshipTarget>()
                .unwrap()
                .collection()
                .iter()
                .filter(|child| !existing_children.contains(child))
                .collect()
        }
        None => c(entity)
            .get::<R::RelationshipTarget>()
            .unwrap()
            .collection()
            .iter()
            .collect(),
    }
}

/// Calls `c` and returns the new child of `entity` from that.
pub(super) fn get_new_related<R: Relationship>(
    entity: &mut EntityWorldMut,
    c: impl for<'a, 'w> FnOnce(&'a mut EntityWorldMut<'w>) -> &'a mut EntityWorldMut<'w>,
) -> Entity {
    match entity.get::<R::RelationshipTarget>() {
        Some(target) => {
            let existing_children: EntityHashSet = target.collection().iter().collect();
            c(entity)
                .get::<R::RelationshipTarget>()
                .unwrap()
                .collection()
                .iter()
                .filter(|child| !existing_children.contains(child))
                .next()
                .unwrap()
        }
        None => c(entity)
            .get::<R::RelationshipTarget>()
            .unwrap()
            .collection()
            .iter()
            .next()
            .unwrap(),
    }
}
