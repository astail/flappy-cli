//! 決定論 RNG（SplitMix64）。
//!
//! OS エントロピーを引かず、`(seed, 呼び出し列)` だけで完全に再現する。
//! seed=0 でも縮退せず初手から散る（素の xorshift の state=0 問題を避ける）。
//! ここでは `Game::new` が最初の棒を生成するのに必要な最小 API のみを置く。
//! 包括的なテスト・追加ヘルパは #4 で拡充する。

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
    pub fn gen_range_inclusive(&mut self, lo: u16, hi: u16) -> u16 {
        let span = (hi - lo) as u64 + 1;
        lo + (self.next_u64() % span) as u16
    }
}
