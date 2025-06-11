use std::{
    any::TypeId,
    hash::{BuildHasher, Hasher},
};

use bevy::{
    ecs::reflect::AppTypeRegistry,
    log::error,
    platform::hash::{FixedHasher, PassHash},
    reflect::ReflectFromPtr,
};

use super::*;

/// Fails if `entity` is `rev_is_despawned`, it must be otherwise spawned as an `archetype_id` could be provided.
pub(super) fn buffer_pre_insert<T: Bundle>(
    world: &mut World,
    now: NonLogNow,
    entity: Entity,
    archetype_id: ArchetypeId,
    insert_mode: InsertMode,
    marker: DisabledToDespawn,
) -> Result<Option<Entity>, RevEntityError> {
    match insert_mode {
        InsertMode::Replace => buffer_components_cached(
            world,
            now,
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world: &mut World| {
                let bundle_id = world.register_bundle::<T>().id();
                pre_insert_maybe_overwrite(world, bundle_id, archetype_id)
            },
            marker,
        ),
        InsertMode::Keep => buffer_components_cached(
            world,
            now,
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                pre_insert_no_overwrite(&world, bundle_id, archetype_id)
            },
            marker,
        ),
    }
}

pub(super) fn pre_insert_maybe_overwrite(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId,
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(2), B(2), C(2), D(2), E(1)

    // Buffer 1:         A(1), B(1), C(*), D(*)        *if any appear at redo
    // Buffer 2 at undo: A(2), B(2), C(2), D(2)

    let bundle = world.bundles().get(bundle_id).unwrap();
    let archetype = world.archetypes().get(archetype_id).unwrap();
    let components = bundle
        .explicit_components()
        .iter()
        .chain(
            bundle
                .required_components()
                .iter()
                .filter(|component_id| !archetype.contains(**component_id)),
        )
        .copied()
        .collect();
    let overwrites = bundle
        .explicit_components()
        .iter()
        .any(|component_id| archetype.contains(*component_id));
    let at = if overwrites {
        BufferAt::NowAndUndo
    } else {
        BufferAt::Undo
    };
    (at, components)
}

pub(super) fn pre_insert_no_overwrite(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId, // todo: remove after https://github.com/bevyengine/bevy/pull/19326
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(1), B(1), C(2), D(2), E(1)

    // Buffer at undo:               C(2), D(2)

    let archetype = world.archetypes().get(archetype_id).unwrap();
    let components = world
        .bundles()
        .get(bundle_id)
        .unwrap()
        .contributed_components()
        .iter()
        .copied()
        .filter(|component_id| !archetype.contains(*component_id))
        .collect();
    (BufferAt::Undo, components)
}

pub(super) fn buffer_components_cached<T: AsRef<[ComponentId]>>(
    world: &mut World,
    now: NonLogNow,
    entity: Entity,
    key: impl Hash + 'static,
    components: impl FnOnce(&mut World) -> (BufferAt, T),
    marker: DisabledToDespawn,
) -> Result<Option<Entity>, RevEntityError> {
    #[derive(Resource, Default)]
    pub(crate) struct CachedBundles(HashMap<u64, (BufferAt, BundleId), PassHash>);

    fn type_id_of_var<T: 'static>(_: &T) -> TypeId {
        TypeId::of::<T>()
    }

    let mut hasher = FixedHasher::default().build_hasher();
    type_id_of_var(&key).hash(&mut hasher);
    key.hash(&mut hasher);
    let key = hasher.finish();

    let mut cache = world.remove_resource::<CachedBundles>().unwrap_or_default();
    let (at, bundle) = *cache.0.entry(key).or_insert_with(|| {
        let (at, components) = components(world);
        let components = components.as_ref();
        (at, components_to_bundle(world, &components))
    });
    world.insert_resource(cache);
    buffer_bundle(world, now, entity, at, bundle, marker)
}

pub(super) fn buffer_bundle(
    world: &mut World,
    now: NonLogNow,
    entity: Entity,
    at: BufferAt,
    bundle: BundleId,
    marker: DisabledToDespawn,
) -> Result<Option<Entity>, RevEntityError> {
    if world.get_entity(entity)?.rev_is_despawned() {
        return Err(RevEntityError::EntityRevDespawnedError(
            EntityRevDespawnedError::new(entity, marker),
        ));
    }
    world.resource_scope::<RevRelationship, _>(|world, mut relationship_res| {
        let mut buffer = BundleBuffer {
            bundle,
            entity,
            state: BufferState::Unspawned(marker),
        };
        match at {
            BufferAt::Now => {
                let entities = buffer.toggle_state(world);
                let components = buffer.get_component_ids(world);
                let out = entities.buffer;
                relationship_res.buffer(
                    &mut world.entity_mut(entity),
                    Some(&components),
                    now,
                    true,
                );
                world.buffer_undo_redo(now, buffer);
                entities.move_components(world, &components, RevDirection::NOT_LOG);
                Ok(Some(out))
            }
            BufferAt::Undo => {
                let components = buffer.get_component_ids(world);
                world.buffer_undo_redo(now, buffer);

                // needs to come after buffer_undo_redo so at undo, at reverse order, this gets to grap relevant components
                relationship_res.buffer(
                    &mut world.entity_mut(entity),
                    Some(&components),
                    now,
                    false,
                );
                Ok(None)
            }
            BufferAt::NowAndUndo => {
                // todo: different double buffer, not two, make use of same EntityCloner
                let at_undo = buffer.clone(); // no buffer entity set yet so each spawns their own
                let entities = buffer.toggle_state(world);
                let components = buffer.get_component_ids(world);

                let out = entities.buffer;
                let mut entity_mut = world.entity_mut(entity);
                let has_relationships =
                    relationship_res.buffer(&mut entity_mut, Some(&components), now, true);
                entity_mut.world_scope(|world| {
                    entities.move_components(world, &components, RevDirection::NOT_LOG);
                    world.buffer_undo_redo(now, [buffer, at_undo]);
                });
                if has_relationships {
                    // needs to come after buffer_undo_redo so at undo, at reverse order, this gets to grap relevant components
                    relationship_res.buffer(&mut entity_mut, Some(&components), now, false);
                }
                Ok(Some(out))
            }
        }
    })
}

/// [`World::buffer_components_in_progress`] returns `Some(direction)` during the execution of the closure.
pub(crate) fn progress_scope(
    world: &mut World,
    progress: BufferInProgress,
    c: impl FnOnce(&mut World),
) {
    let mut swap = ResourceSwap(Some(BufferInProgressRes(progress)));
    swap.undo(world);
    c(world);
    swap.redo(world);
}

#[derive(Clone)]
struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    state: BufferState,
}

#[derive(Copy, Clone)]
enum BufferState {
    Unspawned(DisabledToDespawn),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleEntities {
    fn move_components(
        self,
        world: &mut World,
        components: &[ComponentId],
        direction: RevDirection,
    ) {
        let progress = BufferInProgress::Buffer {
            direction,
            buffer: self.buffer,
        };
        progress_scope(world, progress, |world| {
            EntityCloner::build(world)
                .deny_all()
                .move_components(true)
                .without_required_components(|builder| {
                    builder.allow_by_ids(components.iter().copied());
                })
                .clone_entity(self.source, self.target); // todo: return for 2nd step after overwrite
        })
    }
}

impl BundleBuffer {
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(marker) => {
                let buffer = world.spawn(marker).id();
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
            BufferState::Filled(buffer) => {
                self.state = BufferState::Empty(buffer);
                BundleEntities {
                    target: self.entity,
                    source: buffer,
                    buffer,
                }
            }
            BufferState::Empty(buffer) => {
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
        }
    }
    fn get_component_ids(&self, world: &World) -> Box<[ComponentId]> {
        world
            .bundles()
            .get(self.bundle)
            .expect("todo")
            .explicit_components()
            .into()
    }
    fn undo_redo(&mut self, world: &mut World, direction: RevDirection) {
        let entities = self.toggle_state(world);
        let components = self.get_component_ids(world);
        entities.move_components(world, &components, direction);
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::BackwardLog);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world, RevDirection::FORWARD_LOG);
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BufferInProgress {
    Buffer {
        direction: RevDirection,
        buffer: Entity,
    },
    NonEntityBuffer {
        direction: RevDirection,
    },
    FinalDespawn,
}

impl BufferInProgress {
    pub fn check(world: &World) -> Option<Self> {
        world.get_resource::<BufferInProgressRes>().map(|res| res.0)
    }
    pub fn direction(self) -> RevDirection {
        match self {
            Self::Buffer { direction, .. } => direction,
            Self::NonEntityBuffer { direction } => direction,
            Self::FinalDespawn => RevDirection::NOT_LOG,
        }
    }
}

#[derive(Resource)]
struct BufferInProgressRes(BufferInProgress);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferAt {
    /// The components will be buffered now which removes them from the entity.
    ///
    /// When this is undone, the components are moved back into the entity from the buffer.
    ///
    /// Redoing this results in buffering and removing them from the entity again.
    ///
    /// This variant is useful as reversible removals of these components.
    ///
    /// **Make sure to not manually remove the components and solely use this buffering.**
    Now,
    /// The components will be buffered when this action is undone, which then removed them
    /// from the entity. Until then they remain at the entity.
    ///
    /// When this is redone, the components are moved back into the entity from the buffer.
    ///
    /// This variant is useful to make accompanied insertions of these components _without_
    /// overwrites reversible.
    Undo,
    /// Combines [`BufferAt::Now`] and [`BufferAt::Undo`], utilizing two separate buffers.
    ///
    /// This variant is useful to make accompanied insertions of these components _with_
    /// overwrites reversible.
    ///
    /// **Make sure to do such insertions right after and not before this buffering.**
    NowAndUndo,
}

// todo: replace this with register_dynamic_bundle when moving components no longer requires cloning
pub(super) fn components_to_bundle(world: &mut World, components: &[ComponentId]) -> BundleId {
    #[derive(Resource, Default)]
    struct CheckedClonable(HashSet<ComponentId>);

    let mut checked = world
        .remove_resource::<CheckedClonable>()
        .unwrap_or_else(|| match world.get_resource::<RevRelationship>() {
            Some(resource) => CheckedClonable(resource.registered().collect()),
            None => CheckedClonable::default(),
        });
    // todo: this should be () outside reflect flag
    let registry = world
        .get_resource::<AppTypeRegistry>()
        .map(|registry| registry.read());
    for &component_id in components {
        if !checked.0.insert(component_id) {
            continue;
        }
        let Some(component_info) = world.components().get_info(component_id) else {
            continue;
        };
        let movable = match component_info.clone_behavior() {
            // todo: reflect feature cfg and alternative which returns false
            ComponentCloneBehavior::Default => component_info
                .type_id()
                .zip(registry.as_ref())
                .is_some_and(|(type_id, registry)| {
                    registry
                        .get_type_data::<ReflectFromPtr>(type_id)
                        .is_some_and(|registration| registration.type_id() == type_id)
                }),
            ComponentCloneBehavior::Custom(_) => true, // impls Clone or intentionally does not clone relationship
            ComponentCloneBehavior::Ignore => false,
        };
        if !movable {
            error!(
                "Component {} is unclonable and unreflectable, it's insert, remove or overwrite \
                will not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                component_info.name()
            );
        }
    }
    drop(registry);

    world.insert_resource(checked);

    world.register_dynamic_bundle(components).id()
}

#[macro_export]
macro_rules! unique_for_location {
    ($($hashable: expr),*) => {
        // extra scope to keep `UniquePerInvoke`s isolated
        {
            struct UniquePerInvoke;
            (core::any::TypeId::of::<UniquePerInvoke>(), $($hashable,)*)
        }
    }
}

pub(super) use unique_for_location;

#[cfg(test)]
mod test {
    use bevy::prelude::{Reflect, ReflectDefault, ReflectFromWorld};

    use crate::panic_on_error_events;

    use super::*;

    #[test]
    fn components_to_bundle_does_not_panic_for_clonable_or_reflectable() {
        #[derive(Component, Clone)]
        struct Clonable;

        // following components taken from bevy's clone_entity_using_reflect_all_paths test

        // `reflect_clone`-based fast path
        #[derive(Component, Reflect)]
        #[reflect(from_reflect = false)]
        struct ReflectableA;

        // `ReflectDefault`-based fast path
        #[derive(Component, Reflect, Default)]
        #[reflect(Default)]
        #[reflect(from_reflect = false)]
        struct ReflectableB;

        // `ReflectFromReflect`-based fast path
        #[derive(Component, Reflect)]
        struct ReflectableC;

        // `ReflectFromWorld`-based fast path
        #[derive(Component, Reflect, Default)]
        #[reflect(FromWorld)]
        #[reflect(from_reflect = false)]
        struct ReflectableD;

        panic_on_error_events();
        let mut world = World::new();
        world.init_resource::<AppTypeRegistry>();
        let registry = world.get_resource::<AppTypeRegistry>().unwrap();
        registry
            .write()
            .register::<(ReflectableA, ReflectableB, ReflectableC, ReflectableD)>();
        let clonable_id = world.register_component::<Clonable>();
        let reflectable_a_id = world.register_component::<ReflectableA>();
        let reflectable_b_id = world.register_component::<ReflectableB>();
        let reflectable_c_id = world.register_component::<ReflectableC>();
        let reflectable_d_id = world.register_component::<ReflectableD>();

        components_to_bundle(
            &mut world,
            &[
                clonable_id,
                reflectable_a_id,
                reflectable_b_id,
                reflectable_c_id,
                reflectable_d_id,
            ],
        );

        // test if these really are movable
        let clone = world
            .spawn((
                Clonable,
                ReflectableA,
                ReflectableB,
                ReflectableC,
                ReflectableD,
            ))
            .clone_and_spawn();

        let entity = world.entity(clone);
        assert!(entity.contains::<Clonable>());
        assert!(entity.contains::<ReflectableA>());
        assert!(entity.contains::<ReflectableB>());
        assert!(entity.contains::<ReflectableC>());
        assert!(entity.contains::<ReflectableD>());
    }

    #[test]
    #[should_panic(expected = "MyComponent is unclonable and unreflectable")]
    fn components_to_bundle_errors_on_non_clone_component() {
        #[derive(Component)]
        struct MyComponent;

        panic_on_error_events();
        let mut world = World::new();
        world.init_resource::<AppTypeRegistry>();
        let component_id = world.register_component::<MyComponent>();
        components_to_bundle(&mut world, &[component_id]);
    }
}
