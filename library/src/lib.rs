/*
TODO:

- general todo!() und //todo
- value logs
- log tests
- reversible hooks ? or issue to link
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- App Methoden auch für RevSchedule struct
- tuple impl configs
- derive Reflection + serde für logs
-- register type data for non-pub structs (helper glued to log?)
- longer log tests for RareTransition and RareValue
- tests of other log methods like clear variants
- cfg für ecs only (keine App), logging
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist
- consider renaming packed_uint to compressed_usize
*/

use std::hash::Hash;

use bevy::ecs::{intern::Interned, schedule::ScheduleLabel};

pub mod app; // todo: rename, put app into a sub module and cfg gate it
pub mod commands;
pub mod log;
pub mod meta;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;
