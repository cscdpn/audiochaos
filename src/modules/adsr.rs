use crate::engine::module::{Buffer, Module, ModuleInfo};
use crate::engine::param::{ParamInfo, Scale};

pub const IN_GATE: usize = 0;

pub const OUT: usize = 0;

pub const ATTACK: usize = 0;
pub const DECAY: usize = 1;
pub const SUSTAIN: usize = 2;
pub const RELEASE: usize = 3;

const fn time(name: &'static str, default: f32, help: &'static str) -> ParamInfo {
    ParamInfo {
        name,
        min: 0.001,
        max: 10.0,
        default,
        scale: Scale::Exponential,
        unit: "s",
        help,
    }
}

pub static INFO: ModuleInfo = ModuleInfo {
    name: "adsr",
    description: "Envelope: shapes a note from a gate, rising, falling to a level held with the key, then fading. Into a VCA's mod, or a cutoff.",
    inputs: &["gate"],
    outputs: &["env"],
    output_details: &["0..1"],
    params: &[
        time("attack", 0.005, "Time to rise to full when the gate opens."),
        time("decay", 0.2, "Time to fall from full to the sustain level."),
        ParamInfo {
            name: "sustain",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            scale: Scale::Linear,
            unit: "",
            help: "Level held for as long as the gate stays open.",
        },
        time("release", 0.3, "Time to fade out once the gate closes."),
    ],
};

/// Decay and release are exponential, like an analog envelope, and reach
/// within this fraction of their target in their time: e^-5, under 1%.
const TIME_CONSTANTS: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Attack,
    /// Decay, then sustain once there: the same fall towards `sustain`.
    Decay,
    Release,
}

/// Attack, decay, sustain, release. The attack is a straight rise, and
/// starts from wherever the envelope is, so a note played over a fading one
/// does not click. A gate is open above 0.
pub struct Adsr {
    sample_rate: f32,
    stage: Stage,
    level: f32,
    open: bool,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            stage: Stage::Release,
            level: 0.0,
            open: false,
        }
    }

    /// How far an exponential segment of `seconds` moves in one sample.
    fn step(&self, seconds: f32) -> f32 {
        1.0 - (-TIME_CONSTANTS / (seconds * self.sample_rate)).exp()
    }
}

impl Module for Adsr {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], params: &[Buffer], outputs: &mut [Buffer]) {
        for (i, gate) in inputs[IN_GATE].iter().enumerate() {
            let open = *gate > 0.0;
            if open && !self.open {
                self.stage = Stage::Attack;
            } else if !open && self.open {
                self.stage = Stage::Release;
            }
            self.open = open;
            match self.stage {
                Stage::Attack => {
                    self.level += 1.0 / (params[ATTACK][i] * self.sample_rate);
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.stage = Stage::Decay;
                    }
                }
                Stage::Decay => {
                    let target = params[SUSTAIN][i];
                    self.level += (target - self.level) * self.step(params[DECAY][i]);
                }
                Stage::Release => {
                    self.level -= self.level * self.step(params[RELEASE][i]);
                }
            }
            outputs[OUT][i] = self.level;
        }
    }

    fn led(&self) -> Option<f32> {
        Some(self.level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::module::BLOCK;
    use crate::modules::test_util::run;

    const SR: f32 = 48_000.0;
    const MS: usize = 48;

    /// Attack, decay, sustain, release.
    const SHAPE: [f32; 4] = [0.01, 0.1, 0.5, 0.2];

    /// The envelope over `ms` milliseconds with the gate held at `gate`.
    fn hold(adsr: &mut Adsr, gate: f32, ms: usize) -> Vec<f32> {
        run(adsr, &[gate], &SHAPE, OUT, ms * MS / BLOCK)
    }

    #[test]
    fn a_held_note_rises_falls_to_sustain_and_stays() {
        let mut adsr = Adsr::new(SR);
        let out = hold(&mut adsr, 1.0, 400);
        let peak = out.iter().position(|&l| l == 1.0).unwrap();
        assert!(peak.abs_diff(10 * MS) <= 2, "attack took {peak} samples");
        // Within 1% of sustain once the decay time has passed.
        let after_decay = out[peak + 100 * MS];
        assert!((after_decay - 0.5).abs() < 0.01, "{after_decay}");
        assert!((out[out.len() - 1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn releasing_fades_out_in_the_release_time() {
        let mut adsr = Adsr::new(SR);
        hold(&mut adsr, 1.0, 400);
        let out = hold(&mut adsr, 0.0, 300);
        assert!(out[0] < 0.5 && out[0] > 0.45);
        assert!(out[200 * MS] < 0.5 * 0.01, "{}", out[200 * MS]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn a_new_note_rises_from_where_the_last_one_was() {
        let mut adsr = Adsr::new(SR);
        hold(&mut adsr, 1.0, 400);
        let tail = hold(&mut adsr, 0.0, 50);
        let again = hold(&mut adsr, 1.0, 5);
        assert!(again[0] > tail[tail.len() - 1], "no drop to zero");
        assert!(again.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn letting_go_during_the_attack_releases_from_there() {
        let mut adsr = Adsr::new(SR);
        let rise = hold(&mut adsr, 1.0, 4);
        let fall = hold(&mut adsr, 0.0, 10);
        assert!(fall[0] <= rise[rise.len() - 1]);
        assert!(fall[0] > 0.2, "{}", fall[0]);
    }

    #[test]
    fn the_light_follows_the_envelope() {
        let mut adsr = Adsr::new(SR);
        let out = hold(&mut adsr, 1.0, 400);
        assert_eq!(adsr.led(), Some(out[out.len() - 1]));
    }
}
