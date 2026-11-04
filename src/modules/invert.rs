use crate::engine::module::{Buffer, Module, ModuleInfo};

pub const IN: usize = 0;

pub const OUT_NEGATIVE: usize = 0;
pub const OUT_COMPLEMENT: usize = 1;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "invert",
    description: "Turns a signal upside down: `-in` mirrors it around 0, for ±1 signals like an LFO; `1-in` flips it within 0..1, for gates, envelopes and MIDI knobs, so a gate becomes its NOT.",
    inputs: &["in"],
    outputs: &["-in", "1-in"],
    output_details: &["in × -1", "1 - in"],
    params: &[],
};

/// Two ways to turn a signal upside down, as each kind of signal needs:
/// `-in` keeps a ±1 signal within ±1, `1-in` keeps a 0..1 signal within 0..1.
pub struct Invert;

impl Module for Invert {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        let [negative, complement] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        for (i, &x) in inputs[IN].iter().enumerate() {
            negative[i] = -x;
            complement[i] = 1.0 - x;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn mirrors_around_zero_and_flips_within_zero_to_one() {
        for (x, negative, complement) in [(0.25, -0.25, 0.75), (1.0, -1.0, 0.0), (-0.5, 0.5, 1.5)] {
            assert!(
                run(&mut Invert, &[x], &[], OUT_NEGATIVE, 1)
                    .iter()
                    .all(|&y| y == negative)
            );
            assert!(
                run(&mut Invert, &[x], &[], OUT_COMPLEMENT, 1)
                    .iter()
                    .all(|&y| y == complement)
            );
        }
    }
}
