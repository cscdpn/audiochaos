//! Tap tempo: a tempo from the rhythm of button clicks.

/// A pause this long starts a new tempo: slower than the slowest one a
/// `clock` takes, 20 BPM.
const RESTART_SECONDS: f64 = 3.0;
/// Taps averaged, so one tap a little off barely moves the tempo.
const TAPS_KEPT: usize = 5;

#[derive(Default)]
pub struct Tap {
    /// Times of the latest taps, in seconds, oldest first.
    taps: Vec<f64>,
}

impl Tap {
    /// Records a tap at `time`, in seconds. Returns the tempo in BPM from the
    /// taps so far, once there are two.
    pub fn tap(&mut self, time: f64) -> Option<f32> {
        if self
            .taps
            .last()
            .is_some_and(|&t| time - t > RESTART_SECONDS)
        {
            self.taps.clear();
        }
        self.taps.push(time);
        if self.taps.len() > TAPS_KEPT {
            self.taps.remove(0);
        }
        let intervals = self.taps.len() - 1;
        let span = time - self.taps[0];
        (intervals > 0).then(|| (60.0 * intervals as f64 / span) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_taps_make_a_tempo() {
        let mut tap = Tap::default();
        assert_eq!(tap.tap(10.0), None);
        assert_eq!(tap.tap(10.5), Some(120.0));
    }

    #[test]
    fn taps_are_averaged() {
        let mut tap = Tap::default();
        for time in [0.0, 0.5, 1.0, 1.6] {
            tap.tap(time);
        }
        // Four intervals over two seconds, however uneven.
        assert_eq!(tap.tap(2.0), Some(120.0));
    }

    #[test]
    fn only_the_latest_taps_count() {
        let mut tap = Tap::default();
        for n in 0..10 {
            tap.tap(f64::from(n));
        }
        // Slowing down: the old 60 BPM taps fade out of the average.
        let mut bpm = None;
        for n in 1..=TAPS_KEPT {
            bpm = tap.tap(9.0 + 2.0 * n as f64);
        }
        assert_eq!(bpm, Some(30.0));
    }

    #[test]
    fn a_long_pause_starts_over() {
        let mut tap = Tap::default();
        tap.tap(0.0);
        tap.tap(0.5);
        assert_eq!(tap.tap(10.0), None);
        assert_eq!(tap.tap(11.0), Some(60.0));
    }
}
