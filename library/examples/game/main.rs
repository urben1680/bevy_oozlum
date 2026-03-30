use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
};

use bevy_oozlum::prelude::*;

const ROWS: usize = 8;
const MAX_PAST_LEN: u64 = 70;
const CURRENT_BEVY_VERSION: u64 = 190; // 0.19.0
const WINNING_BEVY_VERSION: u64 = 1000; // 1.00.0
const FRAMERATE_MIN: f64 = 10.0;
const FRAMERATE_MAX: f64 = 66.667;

// This example is a game in which you have to toss a large amount of trash into the ocean.
// Since pollution is bad, you have to undo that to leave the ocean clean but keep your score.
//
// As the log length is limited, your time to undo is limited too.
// Leave behind 10 units of waste and it is game over.
// For every waste tossed, the bevy version number and game speed rises, at 1.0 you win!
//
// To be able to pollute even more, you have 8 rows of water to throw into.

// In this module you can see how to write reversible logic
mod rows;

// And in this how to control the global reversible progression
mod control;

#[cfg(not(feature = "ci-mode"))]
fn main() {
    let mut app = App::new();

    app.add_plugins((
        // Add the RevPlugin to your application.
        //
        // The plugin adds an unpaused RevMeta with a max past length of 1 frame and the
        // run_rev_update system to FixedUpdate. We modify the max past len here. The Plugin also
        // registers a disabling component: RevDespawned.
        //
        // General order of systems:
        // 1. run_rev_update runs in the specified schedule (here FixedUpdate)
        // 2. RevUpdate schedule runs unless paused
        // 3. Reversible systems and sync points, all in the RevSystems set, run in normal or
        //    reversed order, depending on the current RevDirection
        RevPlugin.set_max_past_len(MAX_PAST_LEN),
        rows::plugin,
        control::plugin,
        render::plugin,
        DefaultPlugins.set(render::window_plugin()),
    ))
    .add_systems(
        // You can add regular, non-reversible systems to RevUpdate using the vanilla
        // add_systems API, though in that case they should be ordered relative to the
        // RevSystems set
        RevUpdate,
        despawn_lost_waste.after(RevSystems),
    )
    .init_state::<GameState>()
    .init_resource::<Stats>()
    .insert_resource(Time::<Fixed>::from_hz(FRAMERATE_MIN));

    #[cfg(not(feature = "ci-mode"))]
    app.run();

    #[cfg(feature = "ci-mode")]
    test::test(app);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, States)]
enum GameState {
    // Did not lose nor win yet
    #[default]
    Running,

    // Reached bevy 1.0
    Won,

    // Lost 10 units of waste
    Lost,
}

#[derive(Resource, Default)]
struct Stats {
    // Waste that was lost
    lost: u64,

    // Total waste, including undone
    score: u64,
}

#[derive(Component, Clone, Copy)]
#[component(on_add = increase_score)]
struct Waste {
    // Which row the waste was tossed in
    row: u64,

    // At which RevMeta::now the waste was tossed
    tossed_at: u64,
}

// Increase the score and speed during RevDirection::NotLog, check for winning condition
fn increase_score(mut world: DeferredWorld, _: HookContext) {
    // Hooks sensible to RevDirection must be written as such, here we do not want to react on
    // undo-redo logic, only the initial insertion
    if world
        .get_resource::<RevMeta>()
        .is_some_and(RevMeta::is_running_not_log)
        && *world.resource::<State<GameState>>().get() != GameState::Running
    {
        return;
    }

    let mut stats = world.resource_mut::<Stats>();
    stats.score += 1;
    let score = stats.score;
    const SCORE_MAX: u64 = WINNING_BEVY_VERSION - CURRENT_BEVY_VERSION;

    let mut time = world.resource_mut::<Time<Fixed>>();
    let score_normalized = score as f64 / SCORE_MAX as f64;
    let score_frame_rate = (FRAMERATE_MAX - FRAMERATE_MIN) * score_normalized + FRAMERATE_MIN;

    time.set_timestep_hz(score_frame_rate);

    if score == SCORE_MAX {
        world
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Won);
    }
}

// RevMeta::past_end is increased to prevent the past length to exceed the maximum. Then existing,
// old Waste could get out-of-log, is despawne dhere and brings you closer to the losing condition.
fn despawn_lost_waste(
    meta: Res<RevMeta>,
    waste_query: Query<(Entity, &Waste)>,
    this_state: Res<State<GameState>>,
    mut stats: ResMut<Stats>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    // Only during RevDirection::NotLog RevMeta::past_end could have increased, so skip otherwise
    if meta.is_running_log() {
        return;
    }

    for (entity, &Waste { tossed_at, .. }) in waste_query {
        if tossed_at < meta.past_end() {
            // If you want entities to only exist when their spawn was within the current log,
            // you have to handle the despawn yourself
            commands.entity(entity).despawn();

            if *this_state.get() == GameState::Running {
                stats.lost = (stats.lost + 1).max(10);

                if stats.lost == 10 {
                    next_state.set(GameState::Lost);
                }
            }
        }
    }
}

#[cfg(not(feature = "ci-mode"))]
mod render;

#[cfg(feature = "ci-mode")]
#[doc(hidden)]
mod test;

#[cfg(feature = "ci-mode")]
fn main() {
    test::main()
}
