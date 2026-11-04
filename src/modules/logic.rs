use crate::engine::module::{BLOCK, Buffer, Module, ModuleInfo};

pub const IN_A: usize = 0;
pub const IN_B: usize = 1;

pub const OUT_AND: usize = 0;
pub const OUT_OR: usize = 1;
pub const OUT_XOR: usize = 2;
pub const OUT_NOT: usize = 3;

pub static INFO: ModuleInfo = ModuleInfo {
    name: "logic",
    description: "Logic gates on two signals, each high above 0: `and` while both are, `or` while either is, `xor` while one only is, `not` while `a` is not. Combine gates and sequencer steps into new rhythms.",
    inputs: &["a", "b"],
    outputs: &["and", "or", "xor", "not"],
    output_details: &["0 or 1", "0 or 1", "0 or 1", "0 or 1, not a"],
    params: &[],
};

/// Logic gates. A signal is high above 0, the same threshold as clock ticks
/// and triggers, so a ±1 LFO or a sequencer's `cv` works as well as a gate.
#[derive(Default)]
pub struct Logic {
    /// The last `and`, for the light.
    and: f32,
}

impl Module for Logic {
    fn info(&self) -> &'static ModuleInfo {
        &INFO
    }

    fn process(&mut self, inputs: &[Buffer], _: &[Buffer], outputs: &mut [Buffer]) {
        let [and, or, xor, not] = outputs else {
            unreachable!("the graph allocates one buffer per output")
        };
        let level = |high: bool| if high { 1.0 } else { 0.0 };
        for i in 0..BLOCK {
            let a = inputs[IN_A][i] > 0.0;
            let b = inputs[IN_B][i] > 0.0;
            and[i] = level(a && b);
            or[i] = level(a || b);
            xor[i] = level(a != b);
            not[i] = level(!a);
        }
        self.and = and[BLOCK - 1];
    }

    /// On while `and` is: the output that most needs checking by eye.
    fn led(&self) -> Option<f32> {
        Some(self.and)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::test_util::run;

    #[test]
    fn truth_table() {
        // a, b, and, or, xor, not.
        let table = [
            (0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
            (1.0, 0.0, 0.0, 1.0, 1.0, 0.0),
            (0.0, 1.0, 0.0, 1.0, 1.0, 1.0),
            (1.0, 1.0, 1.0, 1.0, 0.0, 0.0),
        ];
        for (a, b, and, or, xor, not) in table {
            let mut logic = Logic::default();
            for (port, expected) in [(OUT_AND, and), (OUT_OR, or), (OUT_XOR, xor), (OUT_NOT, not)] {
                let out = run(&mut logic, &[a, b], &[], port, 1);
                assert!(out.iter().all(|&y| y == expected), "{a} {b} port {port}");
            }
        }
    }

    #[test]
    fn high_means_above_zero() {
        let mut logic = Logic::default();
        for (a, high) in [(0.01, 1.0), (0.0, 0.0), (-0.7, 0.0), (5.0, 1.0)] {
            assert_eq!(run(&mut logic, &[a, 1.0], &[], OUT_AND, 1)[0], high, "{a}");
        }
    }

    #[test]
    fn the_light_is_and() {
        let mut logic = Logic::default();
        run(&mut logic, &[1.0, 1.0], &[], OUT_AND, 1);
        assert_eq!(logic.led(), Some(1.0));
        run(&mut logic, &[1.0, 0.0], &[], OUT_AND, 1);
        assert_eq!(logic.led(), Some(0.0));
    }
}
