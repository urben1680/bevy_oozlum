use bevy::{
    ecs::{component::ComponentId, entity::Entity, world::DeferredWorld},
    prelude::{Resource, World},
    utils::HashMap,
};

use crate::meta::{Direction, RevMeta};

use super::{buffer_rev_command, InitializedRevCommand};

/*

-- might be possible: apply them at non-log forward (assert!), undo/redo is command logic that does not trigger the hook
-- filter hooks on entities that serve as parking removed components (use extra flag component)
-- filter hooks during log

Können die reversible commands der hooks überhaupt im auslösenden System geloggt werden?
Wenn System1 commands abgearbeitet werden, dann von System2, und dann die Hooks/Observer,
dann ist System::apply_deferred ja schon vorbei?
Eventuell braucht es auch bei ForwardSchedule ein System auf der anderen Seite des sync point
das die commands abholt. Diese dürfen aber nicht in irgendein System landen sondern nach allen
Systemen die diesen sync point nutzen
das benötigt wiederum verschiedene sets wie bei der backward schedule

Eventuell macht es sinn ein eigenes log für hooks zu erstellen das nicht auf RevCommands aufbaut
da HookCommand nicht generisch ist

Werden Commands aus Observers/Hooks im gleichen sync point ausgeführt?
Dann klappt das nicht, wäre aber seltsam da die engine extra DeferredWorld nutzt

Discord chat sagt observer/hooks sind fertig wenn ArcSystem::apply_deferred das system apply_deferred
aufgerufen hat https://discord.com/channels/691052431525675048/742569353878437978/1288082352857153547

Test dazu schreiben, auch mit einem anschließenden zweiten sync point der noop sein sollte
*/

/// The direction the current hook is triggered at.
///
/// # Triggers
///
/// | variant | description |
/// | - | - |
/// | `Forward{log:false}` | Triggered by reversible systems in the forward schedule (non-log). This follows bevy's hook logic. |
/// | `Forward{log:true}` | Triggered by reversible systems in the forward schedule (log). This **does not** follow bevy's hook logic and instead is a reversible command. Still this is triggered right after `IndeterministicForward`, see it's description. |
/// | `BackwardLog` | Triggered by reversible systems in the Backward schedule (log). This **does not** follow bevy's hook logic and instead is a reversible command. For example the `on_remove` hook with this indicates no remove but an undone remove. |
/// | `IndeterministicForward` |
///
/// Reversible systems that trigger a hook during non-log
///
/// Note that reversible logic should done at `Forward` and `BackwardLog`.
/// The indeterministic variants are triggered by the hook itself a
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookDirection {
    /// Triggered by reversible systems in the forward schedule. This follows bevy's hook logic.
    Forward { log: bool },
    /// Triggered by reversible systems in the Backward schedule (log). This **does not** follow
    /// bevy's hook logic and instead is a reversible command. For example, the `on_remove` hook
    /// with this indicates no remove but an insert. If an `on_insert` hook was registered as well,
    /// it will be triggered right before this one with `IndeterministicBackward`.
    BackwardLog,
    /// Triggered by reversible systems in the forward schedule. This follows bevy's hook logic.
    /// Reversible logic should not be done with this variant.
    IndeterministicForward,
    /// Triggered by reversible systems in the backward schedule. This follows bevy's hook logic.
    /// Reversible logic should not be done with this variant.
    IndeterministicBackward,
    /// Triggered at any point outside reversible systems. Note that this will only trigger if either
    /// - [`RevSystemsPlugin`](crate::RevSystemsPlugin) was added with `add_rev_meta_sys_in`
    /// being `Some` or the default value **or**
    /// - [`RevMeta::update_world`] was manually inserted to run the reversible schedules **or**
    /// - Between manually calling [`RevMeta::end_running`] and [`RevMeta::update`]
    ///
    /// Otherwise the hook triggers with one of the deterministic variants, depending on which
    /// reversible schedule last ran.
    NonReversibleSchedule,
}

impl HookDirection {
    fn get_in_hook(world: &DeferredWorld) -> Self {
        match world
            .get_resource::<RevMeta>()
            .and_then(RevMeta::get_direction)
        {
            Some(Direction::Forward { log: false }) => Self::Forward { log: false },
            Some(Direction::Forward { log: true }) => Self::IndeterministicForward,
            Some(Direction::BackwardLog) => Self::IndeterministicBackward,
            None => Self::NonReversibleSchedule,
        }
    }
}

pub struct RevComponentHooks<'a> {
    world: &'a mut World,
    component: ComponentId,
}

#[derive(Resource, Default)]
struct ComponentHooksResource {
    hooks: HashMap<ComponentId, ComponentHookEntry>,
}

#[derive(Default)]
struct ComponentHookEntry {
    on_add: Option<RevComponentHook>,
    on_insert: Option<RevComponentHook>,
    on_remove: Option<RevComponentHook>,
}

pub type RevComponentHook =
    for<'w> fn(_: HookDirection, _: DeferredWorld<'w>, _: Entity, _: ComponentId);

impl<'a> RevComponentHooks<'a> {
    pub fn on_add(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_add.is_some() {
            todo!()
        }
        hooks.on_add = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_add(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnAdd,
                        },
                    )
                }
                // todo: filter on parking entities
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_add
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn on_insert(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_insert.is_some() {
            todo!()
        }
        hooks.on_insert = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_insert(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnInsert,
                        },
                    )
                }
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_insert
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn on_remove(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_remove.is_some() {
            todo!()
        }
        hooks.on_remove = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_remove(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnRemove,
                        },
                    )
                }
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_remove
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
}

struct HookCommand {
    entity: Entity,
    component: ComponentId,
    variant: HookVariant,
}

enum HookVariant {
    OnAdd,
    OnInsert,
    OnRemove,
}

impl HookCommand {
    fn undo_redo(&self, world: DeferredWorld, undo: bool) {
        let direction = if undo {
            HookDirection::BackwardLog
        } else {
            HookDirection::Forward { log: true }
        };
        let hooks = world
            .get_resource::<ComponentHooksResource>()
            .expect("todo")
            .hooks
            .get(&self.component)
            .expect("todo");
        let hook = match self.variant {
            HookVariant::OnAdd => hooks.on_add,
            HookVariant::OnInsert => hooks.on_insert,
            HookVariant::OnRemove => hooks.on_remove,
        };
        hook.expect("todo")(direction, world.into(), self.entity, self.component)
    }
}

impl InitializedRevCommand for HookCommand {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world.into(), true)
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world.into(), false)
    }
}
