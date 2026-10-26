//! Smooth playback of a remote player's poses.
//!
//! Each `PlayerState` carries the sender's frame counter. We keep a short buffer of poses and
//! draw the remote player `DELAY` frames in the past, blending between the two buffered poses
//! around that time. Spacing comes from the sender's frame counter, so uneven packet arrival
//! (jitter, bunching) doesn't show; a late packet only matters if it is later than `DELAY`.

use std::collections::VecDeque;

use crate::net::protocol::frame_after;
use crate::puppet::{Pose, blend};

/// Playback margin in frames (60 per second): three updates at 30 Hz. Covers normal arrival
/// jitter and the 10 ms network poll without adding the old 167 ms to every interaction.
const DELAY: f64 = 6.0;
/// Poses kept (about half a second at 30 Hz).
const CAPACITY: usize = 16;
/// How fast the clock estimate is allowed to drift down per update, in frames.
const DRIFT: f64 = 0.02;
/// Resynchronise when an update is this many frames "earlier" than the estimate
/// (the sender's game slowed down or paused).
const RESYNC: f64 = 30.0;

#[derive(Default)]
pub struct Timeline {
    /// Unwrapped sender times, so playback also crosses the u32 frame-counter boundary.
    poses: VecDeque<(f64, Pose)>,
    last_time: Option<u32>,
    /// Estimated `sender frame - local frame`, taken from the least-delayed update.
    offset: Option<f64>,
}

impl Timeline {
    /// Adds a pose stamped with the sender's frame `time`, received at local frame `now`.
    /// Duplicates and out-of-order updates are dropped.
    pub fn push(&mut self, time: u32, pose: Pose, now: f64) {
        let unwrapped = match (self.last_time, self.poses.back()) {
            (Some(last), Some(&(unwrapped, _))) => {
                if !frame_after(time, last) {
                    return;
                }
                unwrapped + f64::from(time.wrapping_sub(last))
            }
            _ => f64::from(time),
        };
        self.last_time = Some(time);
        let sample = unwrapped - now;
        self.offset = Some(match self.offset {
            Some(o) if sample > o => sample,
            Some(o) if o - sample > RESYNC => sample,
            Some(o) => o - DRIFT,
            None => sample,
        });
        self.poses.push_back((unwrapped, pose));
        if self.poses.len() > CAPACITY {
            self.poses.pop_front();
        }
    }

    /// The pose to draw at local frame `now`.
    pub fn sample(&self, now: f64) -> Option<Pose> {
        let target = now + self.offset? - DELAY;
        let (first, first_pose) = *self.poses.front()?;
        if target <= first {
            return Some(first_pose);
        }
        for (&(ta, a), &(tb, b)) in self.poses.iter().zip(self.poses.iter().skip(1)) {
            if target < tb {
                let t = (target - ta) / (tb - ta);
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
            action: 0,
            status: 0,
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
        // Playback runs DELAY frames behind the estimated sender clock.
        let mut last = tl.sample(DELAY + 6.0).unwrap().position[0];
        for frame in 1..8 {
            let x = tl.sample(DELAY + 6.0 + f64::from(frame)).unwrap().position[0];
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

    #[test]
    fn playback_crosses_the_wrapping_frame_counter() {
        let mut tl = Timeline::default();
        tl.push(u32::MAX - 1, at(0.0), 0.0);
        tl.push(0, at(2.0), 2.0);
        tl.push(2, at(4.0), 4.0);
        // A delayed update from before the wrap must not replace the latest pose.
        tl.push(u32::MAX, at(99.0), 5.0);
        assert_eq!(tl.poses.len(), 3);
        assert!((tl.sample(DELAY + 1.0).unwrap().position[0] - 1.0).abs() < 0.1);
        assert_eq!(tl.sample(1000.0).unwrap().position[0], 4.0);
    }

    #[test]
    fn one_missing_update_still_has_smooth_playback_with_normal_jitter() {
        let mut tl = Timeline::default();
        for time in [0, 2, 4, 8, 10, 12] {
            let jitter = if time == 8 { 1.0 } else { 0.0 };
            tl.push(time, at(time as f32), f64::from(time) + 2.0 + jitter);
        }
        let start = DELAY + 2.0;
        for time in 1..10 {
            let x = tl.sample(start + f64::from(time)).unwrap().position[0];
            assert!((x - time as f32).abs() < 0.2, "{time}: {x}");
        }
    }
}
