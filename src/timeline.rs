//! Smooth playback of a remote player's poses.
//!
//! Each `PlayerState` carries the sender's frame counter. We keep a short buffer of poses and
//! draw the remote player `DELAY` frames in the past, blending between the two buffered poses
//! around that time. Spacing comes from the sender's frame counter, so uneven packet arrival
//! (jitter, bunching) doesn't show; a late packet only matters if it is later than `DELAY`.

use std::collections::VecDeque;

use crate::puppet::{Pose, blend};

/// How far behind the sender we draw, in frames (60 per second): 3+ updates at 30 Hz, so
/// uneven arrival (network jitter, the 10 ms network poll) never runs the buffer dry.
const DELAY: f64 = 10.0;
/// Poses kept (about half a second at 30 Hz).
const CAPACITY: usize = 16;
/// How fast the clock estimate is allowed to drift down per update, in frames.
const DRIFT: f64 = 0.02;
/// Resynchronise when an update is this many frames "earlier" than the estimate
/// (the sender's game slowed down or paused).
const RESYNC: f64 = 30.0;

#[derive(Default)]
pub struct Timeline {
    poses: VecDeque<(u32, Pose)>,
    /// Estimated `sender frame - local frame`, taken from the least-delayed update.
    offset: Option<f64>,
}

impl Timeline {
    /// Adds a pose stamped with the sender's frame `time`, received at local frame `now`.
    /// Duplicates and out-of-order updates are dropped.
    pub fn push(&mut self, time: u32, pose: Pose, now: f64) {
        if self.poses.back().is_some_and(|&(last, _)| time <= last) {
            return;
        }
        let sample = f64::from(time) - now;
        self.offset = Some(match self.offset {
            Some(o) if sample > o => sample,
            Some(o) if o - sample > RESYNC => sample,
            Some(o) => o - DRIFT,
            None => sample,
        });
        self.poses.push_back((time, pose));
        if self.poses.len() > CAPACITY {
            self.poses.pop_front();
        }
    }

    /// The pose to draw at local frame `now`.
    pub fn sample(&self, now: f64) -> Option<Pose> {
        let target = now + self.offset? - DELAY;
        let (first, first_pose) = *self.poses.front()?;
        if target <= f64::from(first) {
            return Some(first_pose);
        }
        for pair in self.poses.iter().collect::<Vec<_>>().windows(2) {
            let (&(ta, a), &(tb, b)) = (pair[0], pair[1]);
            if target < f64::from(tb) {
                let t = (target - f64::from(ta)) / f64::from(tb - ta);
                return Some(blend(&a, &b, t as f32));
            }
        }
        // Ran out of data (packet loss or a stall): hold the newest pose.
        self.poses.back().map(|&(_, p)| p)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> Pose {
        Pose {
            character: 0,
            position: [x, 0.0, 0.0],
            rotation: [0; 3],
            anim: 0,
            frame: 0.0,
            flags: 0,
            upgrades: 0,
        }
    }

    /// Sender moves 1 unit per frame and sends every 2 frames; arrival is jittery.
    #[test]
    fn jitter_does_not_cause_steps() {
        let mut tl = Timeline::default();
        let jitter = [0.0, 3.0, 0.5, 2.5, 0.0, 1.0, 3.0, 0.0];
        for (i, j) in jitter.iter().enumerate() {
            let time = 100 + 2 * i as u32;
            // Base latency 5 frames plus jitter; local clock = sender clock - 100.
            tl.push(time, at(time as f32), f64::from(time - 100) + 5.0 + j);
        }
        // Between updates, playback must advance steadily: one unit per local frame.
        // Playback runs DELAY frames behind: sender frames 100..114 show at local 15..29.
        let mut last = tl.sample(16.0).unwrap().position[0];
        for frame in 17..24 {
            let x = tl.sample(f64::from(frame)).unwrap().position[0];
            assert!(
                (x - last - 1.0).abs() < 1e-3,
                "frame {frame}: {last} -> {x}"
            );
            last = x;
        }
    }

    #[test]
    fn drops_duplicates_and_holds_when_data_runs_out() {
        let mut tl = Timeline::default();
        tl.push(10, at(10.0), 0.0);
        tl.push(10, at(99.0), 0.0);
        tl.push(8, at(99.0), 0.0);
        assert_eq!(tl.poses.len(), 1);
        tl.push(12, at(12.0), 2.0);
        assert_eq!(tl.sample(1000.0).unwrap().position[0], 12.0);
    }

    #[test]
    fn resyncs_after_the_sender_stalls() {
        let mut tl = Timeline::default();
        tl.push(100, at(0.0), 0.0);
        // Sender paused for 2 s: its next frame number is far behind our clock.
        tl.push(102, at(1.0), 120.0);
        assert_eq!(tl.offset, Some(102.0 - 120.0));
    }
}
