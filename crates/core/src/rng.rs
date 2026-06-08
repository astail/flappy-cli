//! 決定論 RNG（SplitMix64）。
//!
//! OS エントロピーを引かず、`(seed, 呼び出し列)` だけで全環境ビット一致で再現する。
//! 整数演算（wrapping add/mul・xor・shift）のみを使い、`f32` の transcendental/FMA を
//! 持ち込まないため native/wasm で同一出力になる（§3 決定論ガードレール）。
//! seed=0 でも縮退せず初手から散る（素の xorshift の state=0 問題を避ける）。
//! seed は呼び出し側が渡す（term: システム時刻、web: `Date.now()`）。

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// SplitMix64 の 1 ステップ。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[lo, hi]`（両端含む）の一様乱数。`lo <= hi` を前提とする。
    ///
    /// core が引く範囲（棒の `gap_top` 等）は span が高々数十なので、剰余法の
    /// modulo bias は 2^64 に対し ~1e-18 と実用上ゼロ。rejection sampling は過剰として採らない。
    pub fn gen_range_inclusive(&mut self, lo: u16, hi: u16) -> u16 {
        debug_assert!(lo <= hi);
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一 seed → 同一列（決定論の核）。
    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// seed=0 でも 0 を連発しない（縮退しない）。
    #[test]
    fn seed_zero_does_not_stick_at_zero() {
        let mut r = Rng::new(0);
        let xs: Vec<u64> = (0..8).map(|_| r.next_u64()).collect();
        assert!(xs.iter().all(|&x| x != 0), "produced a zero: {xs:?}");
        // 連続値がすべて異なる（定数を吐き続けていない）。
        for w in xs.windows(2) {
            assert_ne!(w[0], w[1]);
        }
    }

    /// 既知ベクタによる回帰ガード（定数の取り違え・アルゴリズム破壊を検出）。
    /// SplitMix64(seed=0) の先頭 3 出力。
    #[test]
    fn known_vector_seed_zero() {
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }

    /// 異なる seed は異なる列（衝突しない）。
    #[test]
    fn different_seeds_differ() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    /// gen_range_inclusive は常に範囲内、かつ全値を取りうる。
    #[test]
    fn gen_range_inclusive_within_bounds_and_covers() {
        let mut r = Rng::new(99);
        let (lo, hi) = (3u16, 9u16);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let v = r.gen_range_inclusive(lo, hi);
            assert!((lo..=hi).contains(&v));
            seen.insert(v);
        }
        // 7 値（3..=9）すべてが出現する。
        for v in lo..=hi {
            assert!(seen.contains(&v), "value {v} never produced");
        }
    }

    /// lo == hi のとき常にその値（span=1）。
    #[test]
    fn gen_range_inclusive_singleton() {
        let mut r = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(r.gen_range_inclusive(5, 5), 5);
        }
    }
}
