use std::{io::stdout, num::NonZeroU64, time::Duration};

use bevy::{
    app::App,
    ecs::{entity::EntityHashSet, lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
};

use crossterm::{ExecutableCommand, cursor::*, terminal::*};

use bevy_oozlum::{
    log::{PastLenLog, TransitionLog},
    meta::NonLogNow,
    prelude::*,
};

const MAX_LOG_LEN: u64 = 71;
const FIXED_TIMESTEP: Duration = Duration::from_millis(100);
const CURRENT_BEVY_VERSION: usize = 0_17_0;
const WINNING_BEVY_VERSION: usize = 1_00_0;

// todo: mention how the last column cannot be undone
/*

Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!
No worry, it's okay as long you undo it. Just don't wait for too long...

                         It is Bevy 0.27.3 now!

  ~'`-._,~'`-._,~'`-._,~'`-._,~'1`-._,~'`-._,~'`-._,~'`-._,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'2`-._,#'`-._,~'`-._,~'`-._,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'3`-._,~'`-._,~'`-._,~'`-._,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'4`-._,~'`-._,~'`-._,##`-._,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'5`-._,~'`-._,~'`-._,~'`-.#,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'6`-._,~'`-._,~'`#._,~'`-._,~'`
  ~'`-._,~'`-._,~'`-._,~'`-._,~'7`-._,~'`-._,~'`-._,~'`-._,~'`
                       <- future ^ past ->

1-7: toss waste (007, lost: ##._,~'`-.)          ESC: close
LEFT: forward log, pause at end                  UP: exit log and resume
RIGHT: backward log, pause at end                DOWN: pause

meta.past_end()   == 00000                       meta.past_len()   == 28
meta.now()        == 00028                       meta.len()        == 59
meta.future_end() == 00058                       meta.future_len() == 30

*/

/*
todo: make systems fallible instead of unwrap
*/

fn main() {
    // Ignore this, not imporant for the reversible ECS showcase but for this app.
    let _scope_guard = ScopeGuard::new();

    App::new()
        .insert_resource(RevMeta::new(NonZeroU64::new(MAX_LOG_LEN), false)) // todo fix plugin
        .add_plugins((
            // Add Bevy's MinimalPlugins for this example.
            MinimalPlugins,
            // Add the main plugin to the app. It has different constructors you can choose from.
            // This one here adds the given RevMeta and adds its runner system, which calls
            // RevUpdate, in FixedUpdate. You most likely want to pick this schedule, or another
            // one which is called in FixedUpdate.
            RevPlugin::default(),
            // This example game consists of seven rows you can toss Waste in by pressing 1 to 7.
            // Each row is implemented as a plugin here that adds its system and other things if
            // needed. It makes no sense to give each row a different logic, but they shall show
            // you different features of this crate.
            // Each row system is put into the Row system set with its number to demonstrate
            // ordering further below.
            (row1, row2, row3, row4, row5, row6, row7),
        ))
        // This crate brings rev_configure_sets and rev_add_systems that should be used for
        // reversible systems.
        .rev_configure_sets(
            // RevUpdate is the schedule where you have to put the reversible systems into.
            // You may also add them to another schedule, but that in turn should be run by a
            // reversible system itself which is put into RevUpdate.
            RevUpdate,
            // We dont need the rows to be ordered in a particular way, but that is possible
            // with this crate too: If system A comes before system B, during the backward
            // run of the schedule, this order is reversed. All known system configurations, including
            // via sets, are supported in rev_* variants.
            //
            // Under the hood, this works by having a forward set and a backward set. Each system
            // is put into an Arc and two clones of them are added to each of the two sets. That way
            // they can each share the same system state.
            //
            // The backward set is a bit special as well, as when a system previously issued commands
            // in the forward set, in the backward set the commands will be undone right before the
            // system so your backward logic sees the world like it did just when it finished running
            // forward.
            //
            // Below you see a demonstration of the API. These are all set configurations but they
            // are also available for systems. Be sure to not mix it with Bevy's regular system and
            // set configuration with these. If you must, you can order non-reversible systems and
            // sets to the RevSystems set that the crate adds all reversible systems to.
            (
                Row(1).rev_ambiguous_with_all(),
                Row(2).rev_after(Row(1)),
                (
                    Row(3).rev_after_ignore_deferred(Row(2)),
                    Row(4),
                    Row(5).rev_before(Row(6)),
                    Row(7),
                )
                    .rev_chain(),
            ), /* .in_set(RevSystems), automatically done by this crate */
        )
        .add_systems(
            // The main plugin adds the runner to FixedUpdate, this config shows how you can add
            // other systems that should be ordered relatively to the runner. This runner is
            // RevMeta::try_run_rev_update to be exact.
            FixedUpdate,
            (
                // RevMeta is controlled by this system which obviously should be ordered before
                // RevMeta runs RevUpdate.
                control_rev_meta.before(RevMeta::try_run_rev_update),
                // The system that despawns waste entities when they get out of the view is added
                // after the runner so reversible systems get to influence the presence of these
                // entities.
                // After that, the console output should happen.
                (despawn_waste/*, render*/)
                    .chain()
                    .after(RevMeta::try_run_rev_update),
            ),
        )
        // Before reversible systems run, we want to update the resource containing the pressed keys.
        .add_systems(PreUpdate, map_input)
        // You can also add classic systems  via Bevy's API to RevUpdate. Just make sure to order them
        // to the RevSystems set. In this case we want to clear the previously mentioned resource to
        // not contain any pressed keys.
        .add_systems(RevUpdate, clear_input.after(RevSystems))
        // Init and insert some resources, we want the reversible systems to run every 0.1 seconds.
        .init_resource::<KeysPressed>()
        .init_resource::<WasteCounts>()
        .insert_resource(Time::<Fixed>::from_duration(FIXED_TIMESTEP))
        .run();
}

#[derive(Resource, Default)]
struct KeysPressed {
    direction: Option<RevQueue>,
    num1: bool,
    num2: bool,
    num3: bool,
    num4: bool,
    num5: bool,
    num6: bool,
    num7: bool,
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct Row(u8);

#[derive(Component, Clone, Copy)]
struct Waste {
    tossed_at: u64,
    row: usize,
}

#[derive(Resource, Default)]
struct WasteCounts {
    lost: usize,
    score: EntityHashSet,
}

impl WasteCounts {
    fn score(&self) -> usize {
        (self.score.len() + CURRENT_BEVY_VERSION).min(WINNING_BEVY_VERSION)
    }
    fn update(&mut self, entity: Entity, waste: Waste, meta: &RevMeta) {
        self.score.insert(entity);
        if self.score() < WINNING_BEVY_VERSION && waste.tossed_at < meta.past_end() {
            self.lost += 1;
        }
    }
}

fn control_rev_meta(mut meta: ResMut<RevMeta>, keys: Res<KeysPressed>) {
    match keys.direction {
        Some(queue) => meta.set_queue(queue),
        None => {}
    }
}

fn despawn_waste(
    waste: Query<(Entity, &Waste)>,
    mut counts: ResMut<WasteCounts>,
    meta: Res<RevMeta>,
    mut commands: Commands,
) {
    if meta.get_ran_direction() == Some(RevDirection::NOT_LOG) {
        for (entity, &waste) in waste {
            if !meta.contains(waste.tossed_at) {
                commands.entity(entity).despawn();
            }
            counts.update(entity, waste, &meta);
        }
    }
}

// The most simple way to cause a Waste entity to be reversibly spawned is just to use Commands::rev_spawn.
// Such spawns are logged in the system state and is undone/redone when this system runs during a log phase.
// Many other commands are available in reversible form as well.
fn row1(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(1)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        // RevMeta::non_log_now returns a tiny type that is used in the API to express that
        // currently the RevDirection::NOT_LOG phase is active. This makes it much easier
        // to prevent incorrectly issued reversible commands, in contrast to some additional
        // runtime checks deep inside the crate's implementation and confusing call stacks.
        if pressed.num1
            && let Some(now) = meta.non_log_now()
        {
            commands.rev_spawn(
                now,
                Waste {
                    tossed_at: meta.now(), // the current frame can be received from RevMeta
                    row: 1,
                },
            );
        }
    }
}

// This row works as the previous one but uses direct World access instead of commands.
fn row2(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(2)));

    fn system(world: &mut World) {
        if world.resource::<KeysPressed>().num2
            && let Some(now) = world.non_log_now()
        {
            world.rev_spawn(
                now,
                Waste {
                    tossed_at: now.get(), // the current frame can be received from NonLogNow
                    row: 2,
                },
            );
        }
    }
}

// Both previous variants use the BuffersUndoRedo trait API under the hood that all reversible
// structural changes are using. This example is a variant of row1 but with a manual implementation.
fn row3(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(3)));

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        // The system is noop during log phases.
        let Some(now) = meta.non_log_now() else {
            return;
        };

        // If the key is pressed we want to spawn and log a waste entity.
        if pressed.num3 {
            let waste = Waste {
                tossed_at: meta.now(),
                row: 3,
            };

            // We spawn the entity with the regular bevy API.
            let entity = commands.spawn(waste);

            // See this helper function for the manual approach.
            rev_log_scope_and_buffer_waste_op(entity, waste, now);
        }
    }
}

// One can also use a system-local log to track the spawn of Waste entities to undo/redo that depending on
// the active RevDirection phase.
fn row4(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(4)));

    // We choose a TransitionLog as we understand pressing of a key and the resulting spawn as a
    // transition between a state where a new Waste entity does not exist to the state where it does.
    // It also has to be a sparse log because not every frame the key is pressed.
    fn system(
        meta: Res<RevMeta>,
        pressed: Res<KeysPressed>,
        mut counts: ResMut<WasteCounts>,
        mut log: Local<TransitionLog<Option<Entity>>>,
        mut commands: Commands,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 4,
        };

        // Any logs in the future of the log, for example after going backward in time and resuming
        // the NOT_LOG phase, should be despawned as they are no longer part of our reality as we
        // rewrite the future now! (TODO: rewrite, was in NOT_LOG match previously)
        for entity in log.pre_update_drain(&meta).future().flatten() {
            commands.entity(entity).despawn();
        }

        // Depending on the current RevDirection, the system has different behavior
        match meta.running_direction() {
            // During NOT_LOG we want to react on the pressed keys to spawn Waste entities and to log that.
            RevDirection::NOT_LOG => {
                // Note that, while the despawn_waste system further above already deals with despawning
                // waste entities that are out of log, this system does that itself. This is done because
                // there is a detail about TransitionLog one should be aware of if the popped log
                // entries are used and not just ignored.

                // A quirk of Transition logs is that they need one less log entry compared to State logs.
                // For example if the global log can be up to 3 states, then...
                // - State logs need to have these three states
                // - Transition logs only need to have the two transitions that are used for transitioning
                //   from the first to the second state and from the second to the third state
                //
                // So, the transition log would just pop transitions in push_and_pop_past earlier as the log
                // itself does not need that transition anymore.
                // In our case however this popped transition is used to despawn a Waste entity. But we dont
                // want that to happen a frame earlier as otherwise the waste would vanish before it went past
                // the right edge of the water.
                //
                // That is why we add a + 1 here to compensate that behavior.
                // TODO: remove this
                let past_len = meta.past_len() + 1;

                // If the key is pressed, we spawn another Waste entity.
                // If not, we still need to push a None to advance the log.
                let entity = pressed.num4.then(|| commands.spawn(waste).id());

                // Pushing potential Waste entities may also pop an entity that got out-of-log now.
                // These need to be despawned as they are now past the edge of the screen and cannot come back.
                for entity in log.push_and_drain_past(past_len, entity).flatten() {
                    commands.entity(entity).despawn();

                    // The player missed undoing this littering in time and the lost waste counter is increased.
                    counts.update(entity, waste, &meta);
                }
            }

            // When we go backward in log, we just remove the component from the entity that got spawned
            // this frame. When we go forward in log, we reinsert the component.
            // This way we dont need to despawn and respawn during the log phase.
            RevDirection::BackwardLog => {
                if let Some(entity) = log.backward_log().unwrap() {
                    commands.entity(*entity).remove::<Waste>();
                }
            }
            RevDirection::FORWARD_LOG => {
                if let Some(entity) = log.forward_log().unwrap() {
                    commands.entity(*entity).insert(waste);
                }
            }
        }
    }
}

// This row works as the previous row but we want to demonstrate it with using a reversible running condition.
fn row5(app: &mut App) {
    app.rev_add_systems(
        RevUpdate,
        // The system will now only run when the key is pressed. The state of the condition system will make
        // sure to pass at the correct log phases as well. The actual key press only matters for
        // RevDirection::NOT_LOG.
        spawn_and_log_system
            .rev_run_if(spawn_condition)
            .rev_in_set(Row(5)),
    );

    fn spawn_condition(pressed: Res<KeysPressed>) -> bool {
        pressed.num5
    }

    // Other than the previous row, the used log does not need to support Option updates - every system
    // run will cause a spawn so we pick DenseTransitionLog.
    // However, because the past is "shorter" from the point of view of this system, we need to add a
    // FrameTransitionLog to track the exact past length. This may not be needed if you dont care for the
    // entity log length here, then the RevMeta::past_len is fine. We do it here for demonstration purpose.
    fn spawn_and_log_system(
        meta: Res<RevMeta>,
        mut past_len_log: Local<PastLenLog>,
        mut entity_log: Local<TransitionLog<Entity>>,
        mut commands: Commands,
        mut debug: Local<Vec<(RevMeta, PastLenLog)>>,
    ) {
        let waste = Waste {
            tossed_at: meta.now(),
            row: 5,
        };

        past_len_log.pre_update(&meta);
        for entity in entity_log.pre_update_drain(&meta).future() {
            commands.entity(entity).despawn();
        }

        match meta.running_direction() {
            // Note that no despawns happen in this system as the system does not necessarily run when an entity
            // spawned from here gets out-of-log.
            // Not need to check if the key is pressed before the spawn, it is implied by the system running.
            RevDirection::NOT_LOG => {
                // During RevDirection::NOT_LOG, this is always Some.
                let now = meta.non_log_now().unwrap();

                // We get the past len from the frame log instead from RevMeta.
                // Note that here, in contrast to the previous row, we do not need to increase the past_len because
                // we dont do anything with the entities that go out of log.
                let past_len = past_len_log.update_and_get_past_len(&meta);

                // We spawn the waste entity and mark is as log scoped to be despawned when out-of-log.
                let entity = commands.spawn(waste).rev_log_scope(now).id();

                // We do not use the push_and_pop_past method because, as this system does not run every frame,
                // multiple log entries may be out of log now.
                entity_log.push_and_drain_past(past_len, entity);
            }

            // As before, the log behavior is just removing and readding the Waste component.
            RevDirection::FORWARD_LOG => {
                // We also need to update the frame log. As the frame log is advanced every time this system runs,
                // this method should always return true as well because the run condition makes sure it only runs
                // in these frames, also during the log.
                let _true = past_len_log.forward_log(&meta);
                let entity = *entity_log.forward_log().unwrap();
                commands.entity(entity).insert(waste);
            }
            RevDirection::BackwardLog => {
                let _true = past_len_log.backward_log(&meta);
                let entity = *entity_log.backward_log().unwrap();
                commands.entity(entity).remove::<Waste>();
            }
        }
        //debug.push((meta.clone(), past_len_log.clone()));
    }
}

// Hooks can be reversible as well.
fn row6(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(5)))
        .world_mut()
        .register_component_hooks::<Waste>()
        .on_add(on_add);

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if pressed.num6
            && let Some(now) = meta.non_log_now()
        {
            commands.spawn(Waste {
                tossed_at: now.get(),
                row: 6,
            });
        }
    }

    // As hooks and observers also run for entities that this crate uses to backup components in, you might
    // want to differentiate them from usual entities. For that the RevOpInProgress resource is helpful that
    // only exists during hooks and observers.
    fn on_add(mut world: DeferredWorld, context: HookContext) {
        match world.get_resource::<RevOp>() {
            // RevOp::Buffer is present when a component is moved from or to these internal
            // entities. That can happen when you call EntityCommands::rev_remove as then the
            // components are not really removed but moved to a buffer entity. That is how
            // this crate is able to return it when this is undone.
            // This match arm is noop here as we dont want to react on such operations where
            // buffer entities are involved.
            //
            // If we were interested in these cases, "direction" contains the current
            // RevDirection and "buffer" the buffer entity so we could compare it with the entity
            // in HookContext::entity.
            Some(RevOp::Buffer {
                direction: _,
                buffer: _,
            }) => {}

            // RevOp::FinalDespawn is present when an entity is finally despawning. Finally
            // in this case because using EntityCommands::rev_despawn will actually not despawn
            // the entity immediately but instead just clears it from components and disables it.
            // This way the Entity id is still the same for when this despawn is undone.
            // Only when the despawn is impossible to be undone, like when the command gets
            // out-of-log, the entity is finally despawned.
            // However, as said, entites are cleared on EntityCommands::rev_despawn, so hooks and
            // observers targeting the components will most likely not react on the despawn of
            // your entity but on the despawn of the internal buffer entity that received the
            // cleared components.
            // EntityCommands::rev_despawn aside, this RevOp variant is present for any buffer
            // entity that is despawned. Another example is EntityCommands::rev_remove again when
            // the command got out-of-log and the buffer entity that received the removed
            // components is despawned, making the removed components unrecoverable after that.
            //
            // The variable "buffer" here is a boolean and indicates if HookContext::entity is
            // a buffer entity or not.
            Some(RevOp::FinalDespawn { buffer: _ }) => {}

            // No buffer or delayed despawn involved, this is the case we care for!
            None => {
                // Skip when this is not RevDirection::NOT_LOG, which may be the case even when
                // the RevOp resource is absent, like when this hook runs outside the RevUpdate
                // schedule.
                let Some(now) = world.non_log_now() else {
                    return;
                };

                let entity = context.entity;
                let waste = *world.entity(entity).get::<Waste>().unwrap();

                // We only want this hook to be in effect for this specific row.
                if waste.row != 6 {
                    return;
                }

                let mut commands = world.commands();
                let entity = commands.entity(entity);
                rev_log_scope_and_buffer_waste_op(entity, waste, now);
            }
        }
    }
}

// And observer can also be reversible.
fn row7(app: &mut App) {
    app.rev_add_systems(RevUpdate, system.rev_in_set(Row(7)))
        .add_observer(observer);

    #[derive(Event)]
    struct WasteObserverEvent(NonLogNow);

    fn system(meta: Res<RevMeta>, pressed: Res<KeysPressed>, mut commands: Commands) {
        if pressed.num7
            && let Some(now) = meta.non_log_now()
        {
            commands.trigger(WasteObserverEvent(now));
        }
    }

    // As the observer reacts on a custom event and not on the add or remove of some
    // component, the RevOp resource will usually not be present, certainly not in this
    // example. But it may be if the observer is triggered in a hook like in row6.
    fn observer(trigger: On<WasteObserverEvent>, mut world: DeferredWorld) {
        let now = trigger.0;

        let waste = Waste {
            tossed_at: now.get(),
            row: 7,
        };

        let mut commands = world.commands();
        let entity = commands.spawn(waste);
        rev_log_scope_and_buffer_waste_op(entity, waste, now);
    }
}

// Helper function, see inner comments:
fn rev_log_scope_and_buffer_waste_op(
    mut entity_commands: EntityCommands,
    waste: Waste,
    now: NonLogNow,
) {
    // We mark the entity as log scoped which means it will be despawned when this frame gets out of log.
    entity_commands.rev_log_scope(now);

    let entity = entity_commands.id();

    // Types implementing UndoRedo are stored in the system state that is associated with whatever this
    // rev_log_scope_and_buffer_waste_op calls. You can implement it for any of your typed but you can
    // also pass in a closure in like this.
    // This is how all reversible structural operations like commands work, though you can use it for
    // anything else as well.
    // This closure in particular will remove the Waste component on undo and add it back on redo.
    entity_commands.buffer_undo_redo(now, move |world: &mut World, variant: UndoRedoDirection| {
        let mut entity = world.entity_mut(entity);
        match variant {
            UndoRedoDirection::Undo => {
                entity.remove::<Waste>();
            }
            UndoRedoDirection::Redo => {
                entity.insert(waste);
            }
        };
    });
}

/*
    THE FOLLOWING CODE IS NOT RELEVANT TO THE REVERSIBLE ECS EXAMPLE
    Only input logic and console output is done below.
*/

fn map_input(
    mut keys: ResMut<KeysPressed>,
    counts: Res<WasteCounts>,
    mut exit: EventWriter<AppExit>,
) {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, poll, read};

    let mut f = || -> std::io::Result<()> {
        // check if event exists to read, do not block thread to wait for one if not
        if !poll(Duration::from_secs(0))? {
            return Ok(());
        }
        // check if the event is a pressed key
        let Event::Key(KeyEvent {
            kind: KeyEventKind::Press,
            code,
            ..
        }) = read()?
        else {
            return Ok(());
        };
        match code {
            KeyCode::Esc => {
                exit.write(AppExit::Success);
            }

            // ignore any other inputs at game over
            _ if counts.lost >= 10 => {}
            _ if counts.score() >= WINNING_BEVY_VERSION => {}

            KeyCode::Left => keys.direction = Some(RevQueue::Run(RevDirection::FORWARD_LOG)),
            KeyCode::Right => keys.direction = Some(RevQueue::Run(RevDirection::BackwardLog)),
            KeyCode::Up => keys.direction = Some(RevQueue::Run(RevDirection::NOT_LOG)),
            KeyCode::Down => keys.direction = Some(RevQueue::Pause),
            KeyCode::Char('1') => keys.num1 = true,
            KeyCode::Char('2') => keys.num2 = true,
            KeyCode::Char('3') => keys.num3 = true,
            KeyCode::Char('4') => keys.num4 = true,
            KeyCode::Char('5') => keys.num5 = true,
            KeyCode::Char('6') => keys.num6 = true,
            KeyCode::Char('7') => keys.num7 = true,
            _ => {}
        }
        Ok(())
    };

    if enable_raw_mode().is_ok() {
        let _ = f();
        let _ = disable_raw_mode();
    }
}

fn clear_input(mut keys: ResMut<KeysPressed>) {
    *keys = default();
}

struct ScopeGuard;

impl ScopeGuard {
    fn new() -> Self {
        use bevy::log::{
            Level,
            tracing::{Event, Subscriber, dispatcher::get_default},
            tracing_subscriber::{
                Layer,
                layer::{Context, SubscriberExt},
                registry,
                util::SubscriberInitExt,
            },
        };

        struct PanicOnError;

        impl<S: Subscriber> Layer<S> for PanicOnError {
            fn on_event(&self, event: &Event, _ctx: Context<S>) {
                if *event.metadata().level() == Level::ERROR {
                    // Nothing in this example should cause errors.
                    // But to make the example also work to test changes to the crate,
                    // we want errors to cause panics.
                    panic!("{event:#?}")
                }
            }
        }

        if registry().with(PanicOnError).try_init().is_err() {
            get_default(|subscriber| {
                assert!(subscriber.downcast_ref::<PanicOnError>().is_some());
            })
        }

        //let _ = stdout().execute(SetSize(MAX_LOG_LEN as u16 + 2, 13));
        //let _ = stdout().execute(Hide);
        Self
    }
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        let _ = stdout().execute(MoveToRow(14));
        let _ = stdout().execute(Show);
    }
}

fn render(
    meta: Res<RevMeta>,
    waste: Query<&Waste>,
    counts: Res<WasteCounts>,
    mut last_future_end: Local<Option<u64>>,
) {
    let _ = stdout().execute(BeginSynchronizedUpdate);
    let _ = stdout().execute(Clear(ClearType::All));

    let wave = |phase: u64| "`-._,~'".chars().cycle().skip(7 - phase as usize % 7);

    let row_future: String = wave(meta.future_end())
        .take(meta.future_len() as usize)
        .collect();

    let row_past: String = wave(meta.future_end())
        .skip(meta.future_len() as usize % 7)
        .take(meta.past_len() as usize + 1)
        .collect();
    let mut past_rows: [String; 7] = std::array::from_fn(|_| row_past.clone());

    let padding_cols = match *last_future_end {
        Some(frame) => {
            let mut padding = frame.wrapping_sub(meta.future_end());
            if padding > MAX_LOG_LEN {
                padding = 0;
                *last_future_end = Some(meta.future_end());
            }
            padding as usize
        }
        None => {
            *last_future_end = Some(meta.future_end());
            0
        }
    };
    let padding = " ".repeat(padding_cols);

    for Waste { row, tossed_at } in waste.iter().copied() {
        let Some(index) = meta.now().checked_sub(tossed_at) else {
            panic!("now {} < tossed_at {tossed_at} of row {row}", meta.now());
        };
        let index = index as usize;
        past_rows
            .get_mut(row - 1)
            .unwrap()
            .replace_range(index..=index, "#");
    }

    let mut bevy_version = format!("{:04}", counts.score());
    bevy_version.insert(3, '.');
    bevy_version.insert(1, '.');

    println!();
    println!("Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!");
    println!("No worry, it's okay as long you undo it. Just don't wait for too long...");
    println!();
    println!("                         It is Bevy {bevy_version} now!                         ");
    println!();

    for (i, past_row) in past_rows.into_iter().enumerate() {
        println!("{padding}{row_future}{}{past_row}", i + 1);
    }

    let total = waste.iter().len() + counts.lost;
    let lost = counts.lost.min(10);
    let lost_bar: String = "#"
        .repeat(lost)
        .chars()
        .chain(wave(meta.now()).take(10 - lost))
        .collect();

    let marker = "                                                             <- future ^ past ->"
        .chars()
        .skip(MAX_LOG_LEN as usize - (padding_cols + meta.future_len() as usize + 1))
        .take(MAX_LOG_LEN as usize + 1)
        .collect::<String>();

    println!("{marker}");
    println!();

    if meta.get_ran_direction() != Some(RevDirection::NOT_LOG) || lost == 10 {
        println!("                ({total:03}, lost: {lost_bar})          ESC: close");
    } else {
        println!("1-7: toss waste ({total:03}, lost: {lost_bar})          ESC: close");
    }

    let blink = (meta.now() / 8) % 2 == 0;
    if lost < 10 {
        if counts.score() < WINNING_BEVY_VERSION {
            println!("LEFT: forward log, pause at end                  UP: exit log and resume");
            println!("RIGHT: backward log, pause at end                DOWN: pause");
        } else {
            println!();
            if blink {
                println!("                    Yay, Bevy 1.0 is there! YOU WON!");
            } else {
                println!();
            }
        }
    } else {
        println!();
        if blink {
            println!("You left too much waste behind that you can no longer recover. GAME OVER");
        } else {
            println!();
        }
    }

    println!();
    println!(
        "meta.past_end()   == {:05}                       meta.past_len()   == {:02}",
        meta.past_end(),
        meta.past_len()
    );
    println!(
        "meta.now()        == {:05}                       meta.len()        == {:02}",
        meta.now(),
        meta.len()
    );
    println!(
        "meta.future_end() == {:05}                       meta.future_len() == {:02}",
        meta.future_end(),
        meta.future_len()
    );
    println!();

    let _ = stdout().execute(MoveTo(0, 0));
    let _ = stdout().execute(EndSynchronizedUpdate);
}
