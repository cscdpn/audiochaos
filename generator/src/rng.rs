//! Reproducible randomness: the same input gives the same numbers on every
//! platform and with every Rust version. Hence algorithms written out here
//! rather than `std`'s hasher or the `rand` crate, which promise neither.

/// The input's bytes as a seed, with FNV-1a 64: short, specified, and fine
/// for anything from an empty string to a whole file.
pub fn seed(input: &[u8]) -> u64 {
    input.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// SplitMix64. Its output mixing spreads seeds that differ in a single bit
/// over every bit, which FNV-1a alone does not for inputs that differ in
/// their last byte.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 0 to `n - 1`. `n` must not be 0.
    pub fn below(&mut self, n: usize) -> usize {
        // The high bits of a 64 × 64 product: no division, and the bias
        // over a few dozen choices is far below anything audible.
        ((u128::from(self.next_u64()) * n as u128) >> 64) as usize
    }

    /// `min` to `max`, both included.
    pub fn int(&mut self, min: u32, max: u32) -> u32 {
        min + self.below((max - min + 1) as usize) as u32
    }

    /// `min` to `max` in thousandths, so a knob reads well in the file.
    /// Drawn as an integer: no floating-point arithmetic before the one
    /// division, which IEEE rounds the same everywhere.
    pub fn value(&mut self, min: f32, max: f32) -> f32 {
        let (min, max) = ((min * 1000.0).round() as i64, (max * 1000.0).round() as i64);
        (min + self.below((max - min + 1) as usize) as i64) as f32 / 1000.0
    }

    /// True `percent` times out of a hundred.
    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_follow_the_fnv_1a_reference() {
        assert_eq!(seed(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(seed(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(seed(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn numbers_follow_the_splitmix64_reference() {
        let mut rng = Rng::new(0);
        assert_eq!(rng.next_u64(), 0xe220_a839_7b1d_cdaf);
        assert_eq!(rng.next_u64(), 0x6e78_9e6a_a1b9_65f4);
    }

    #[test]
    fn draws_stay_in_their_range_and_reach_both_ends() {
        let mut rng = Rng::new(1);
        let ints: Vec<u32> = (0..1000).map(|_| rng.int(3, 6)).collect();
        assert!(ints.iter().all(|i| (3..=6).contains(i)));
        assert!(ints.contains(&3) && ints.contains(&6));
        let values: Vec<f32> = (0..5000).map(|_| rng.value(-0.5, 0.5)).collect();
        assert!(values.iter().all(|v| (-0.5..=0.5).contains(v)));
        assert!(values.contains(&-0.5) && values.contains(&0.5));
    }

    #[test]
    fn values_are_in_thousandths() {
        let mut rng = Rng::new(2);
        for _ in 0..1000 {
            let v = rng.value(0.0, 2.0);
            assert_eq!(v, (v * 1000.0).round() / 1000.0);
        }
    }
}
