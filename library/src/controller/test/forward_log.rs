/*
- forward log / backward log
-- keeps running at this progress
-- calls commands as expected
-- stops at the end end and changes to log pause
-- triggers log end when reacting on non-log progress query (test all)
-- reacts other log progresses immediately without triggering log end (test all)
*/

use std::num::Wrapping;

use crate::controller::{consts::CONTROLLER_CONSTS, progress::Progress};

use super::{Test, TestAssert, TestControl};

const PROGRESS_FORWARD_FAST_TO_3: [TestControl; 1] = [TestControl {
    progress_query: Some(Progress::ForwardFast {
        to_time_stamp: Wrapping(3),
    }),
    time_step_query: None,
}];