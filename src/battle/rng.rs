//! The battle's random numbers: a small seeded generator (xorshift64*), so one seed always
//! replays the same battle.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // SplitMix64 spreads close seeds apart; the state must not be 0.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng { state: z.max(1) }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    /// 0..n (0 for n = 0).
    pub fn below(&mut self, n: u32) -> u32 {
        ((u64::from(self.next_u32()) * u64::from(n)) >> 32) as u32
    }

    /// True `percent` times in 100.
    pub fn percent(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }

    /// A seed for another generator (the AI's), so it doesn't share this one's sequence.
    pub fn fork(&mut self) -> Rng {
        let hi = u64::from(self.next_u32());
        Rng::new(hi << 32 | u64::from(self.next_u32()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_numbers() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let mut c = Rng::new(8);
        let xs: Vec<u32> = (0..32).map(|_| a.next_u32()).collect();
        let ys: Vec<u32> = (0..32).map(|_| b.next_u32()).collect();
        let zs: Vec<u32> = (0..32).map(|_| c.next_u32()).collect();
        assert_eq!(xs, ys);
        assert_ne!(xs, zs);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut r = Rng::new(0);
        let mut seen = [0u32; 16];
        for _ in 0..16_000 {
            let v = r.below(16);
            seen[v as usize] += 1;
        }
        assert!(seen.iter().all(|&n| (800..1200).contains(&n)), "{seen:?}");
        assert_eq!(r.below(0), 0);
        let hits = (0..10_000).filter(|_| r.percent(25)).count();
        assert!((2200..2800).contains(&hits), "{hits}");
    }
}
