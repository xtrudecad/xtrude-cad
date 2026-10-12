// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! A seeded MT19937 generator with a fixed seeding and fixed helpers, so
//! the seeded ray jitter of the orientation pass always draws the same
//! numbers — its tolerances and fuzz regressions were hardened around
//! those seeds.

const N: usize = 624;
const M: usize = 397;

#[derive(Clone)]
pub struct Mt19937 {
    mt: [u32; N],
    index: usize,
}

impl Mt19937 {
    /// A generator for a non-negative integer seed.
    pub fn new(seed: u64) -> Self {
        // init_by_array over the 32-bit words of the seed, low word first
        let mut key: Vec<u32> = Vec::new();
        let mut s = seed;
        if s == 0 {
            key.push(0);
        }
        while s > 0 {
            key.push((s & 0xffff_ffff) as u32);
            s >>= 32;
        }
        let mut r = Mt19937 {
            mt: [0; N],
            index: N,
        };
        r.init_by_array(&key);
        r
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19_650_218);
        let mut i = 1usize;
        let mut j = 0usize;
        let klen = key.len().max(1);
        let mut k = N.max(klen);
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(*key.get(j).unwrap_or(&0))
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= klen {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000;
        self.index = N;
    }

    fn genrand_u32(&mut self) -> u32 {
        if self.index >= N {
            for kk in 0..N {
                let y = (self.mt[kk] & 0x8000_0000) | (self.mt[(kk + 1) % N] & 0x7fff_ffff);
                let mut v = self.mt[(kk + M) % N] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_b0df;
                }
                self.mt[kk] = v;
            }
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// A double in [0, 1) with 53 random bits.
    pub fn random(&mut self) -> f64 {
        let a = (self.genrand_u32() >> 5) as f64;
        let b = (self.genrand_u32() >> 6) as f64;
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// A double between `a` and `b`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }

    /// `k <= 32` random bits (the top bits of one draw).
    pub fn getrandbits(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        self.genrand_u32() >> (32 - k.min(32))
    }

    /// An integer in `0..n` (rejection sampling over [`Self::getrandbits`]).
    pub fn randbelow(&mut self, n: usize) -> usize {
        if n <= 1 {
            return 0;
        }
        let k = usize::BITS - n.leading_zeros();
        loop {
            let r = self.getrandbits(k) as usize;
            if r < n {
                return r;
            }
        }
    }

    /// An integer in `a..=b`.
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        a + self.randbelow((b - a + 1) as usize) as i64
    }

    /// The index of a random element of a sequence of `len`.
    pub fn choice_index(&mut self, len: usize) -> usize {
        self.randbelow(len)
    }

    /// Shuffle `x` in place (Fisher–Yates, from the end).
    pub fn shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let j = self.randbelow(i + 1);
            x.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECT_SHUFFLE: [i32; 10] = [8, 3, 1, 4, 7, 0, 9, 6, 2, 5];
    const EXPECT_RANDINT: i64 = 3;
    const EXPECT_CHOICE: usize = 1;

    #[test]
    fn integer_helpers_match_the_reference_values() {
        let mut r = Mt19937::new(7);
        let mut x: Vec<i32> = (0..10).collect();
        r.shuffle(&mut x);
        let ri = r.randint(1, 3);
        let ci = r.choice_index(6);
        assert_eq!(
            (x, ri, ci),
            (EXPECT_SHUFFLE.to_vec(), EXPECT_RANDINT, EXPECT_CHOICE)
        );
    }

    #[test]
    fn sequence_matches_the_reference_values() {
        let mut r = Mt19937::new(12345);
        let got: Vec<f64> = (0..3).map(|_| r.random()).collect();
        let want = [
            0.41661987254534116,
            0.010169169457068361,
            0.8252065092537432,
        ];
        for (g, w) in got.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-15, "{g} != {w}");
        }
    }
}
