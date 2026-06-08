//! flappy-core: 純粋なゲームロジック（I/O 依存ゼロ）。
//!
//! `tick()` 駆動の決定論的な状態機械。物理は常に固定 [`DT`] で進む。
//! 本 issue (#5) では型定義・[`Config`] デフォルト・[`Game::new`] と
//! ゲッタ雛形までを用意する。物理 `tick` / 衝突 / スコアは後続 issue で追加する。

mod rng;

use rng::Rng;

/// 物理の固定タイムステップ（秒）。レンダラはアキュムレータで `tick()` 回数を制御する。
pub const DT: f32 = 1.0 / 60.0;

/// チューニング値の集約。デフォルトは DESIGN §7 の初期値。
pub struct Config {
    pub cols: u16,
    pub rows: u16,
    pub bird_col: f32,
    /// 重力（行/秒^2）
    pub gravity: f32,
    /// フラップの上向き初速（負値）
    pub flap_impulse: f32,
    /// スクロール速度（列/秒）
    pub scroll_speed: f32,
    /// 隙間の縦幅（行）
    pub pipe_gap: u16,
    /// 棒の間隔（列）
    pub pipe_spacing: f32,
    /// 終端速度（下向き上限、行/秒）
    pub vy_max: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cols: 64,
            rows: 24,
            bird_col: 12.0,
            gravity: 45.0,
            flap_impulse: -16.0,
            scroll_speed: 12.0,
            pipe_gap: 6,
            pipe_spacing: 22.0,
            vy_max: 30.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Playing,
    GameOver,
}

pub struct Pipe {
    pub x: f32,
    pub gap_top: u16,
    pub passed: bool,
}

// 物理 `tick` を持つ #6 以降でこれらのフィールドが読まれる。現状は初期化のみで
// 未読のため、骨格段階の dead_code 警告を抑止する（後続 issue で解除予定）。
#[allow(dead_code)]
pub struct Game {
    cfg: Config,
    rng: Rng,
    phase: Phase,
    bird_y: f32,
    bird_vy: f32,
    pipes: Vec<Pipe>,
    /// 次の棒生成までの距離
    dist_to_next: f32,
    pub score: u32,
    pub best: u32,
}

impl Game {
    pub fn new(cfg: Config, seed: u64) -> Self {
        // 1 フレーム 1 セル以内を保証する不変条件（スイープ判定を不要にする）。
        assert!(
            cfg.scroll_speed * DT < 1.0 && cfg.vy_max * DT < 1.0,
            "config violates per-frame 1-cell invariant"
        );

        let mut rng = Rng::new(seed);
        // 鳥は画面中央付近から開始。
        let bird_y = cfg.rows as f32 / 2.0;
        // 最初の棒は右端（x = cols）に 1 本だけ。鳥に届くまで約 1 画面ぶんの助走になり開始即死を防ぐ。
        let gap_top = rng.gen_range_inclusive(1, cfg.rows - 1 - cfg.pipe_gap);
        let pipes = vec![Pipe {
            x: cfg.cols as f32,
            gap_top,
            passed: false,
        }];
        let dist_to_next = cfg.pipe_spacing;

        Self {
            cfg,
            rng,
            phase: Phase::Ready,
            bird_y,
            bird_vy: 0.0,
            pipes,
            dist_to_next,
            score: 0,
            best: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_in_ready() {
        let g = Game::new(Config::default(), 1);
        assert_eq!(g.phase(), Phase::Ready);
        assert_eq!(g.score, 0);
        assert_eq!(g.best, 0);
    }

    #[test]
    fn new_satisfies_invariant() {
        // デフォルト Config で不変条件が破れず panic しない。
        let cfg = Config::default();
        assert!(cfg.scroll_speed * DT < 1.0);
        assert!(cfg.vy_max * DT < 1.0);
        let _ = Game::new(cfg, 0);
    }

    #[test]
    fn new_spawns_one_pipe_at_right_edge() {
        let cfg = Config::default();
        let cols = cfg.cols;
        let pipe_gap = cfg.pipe_gap;
        let rows = cfg.rows;
        let spacing = cfg.pipe_spacing;
        let g = Game::new(cfg, 42);
        assert_eq!(g.pipes.len(), 1);
        assert_eq!(g.pipes[0].x, cols as f32);
        assert!(!g.pipes[0].passed);
        let gap_top = g.pipes[0].gap_top;
        assert!((1..=rows - 1 - pipe_gap).contains(&gap_top));
        assert_eq!(g.dist_to_next, spacing);
    }

    #[test]
    #[should_panic(expected = "invariant")]
    fn new_panics_when_invariant_violated() {
        let cfg = Config {
            scroll_speed: 120.0, // 120 * (1/60) = 2.0 >= 1.0
            ..Config::default()
        };
        let _ = Game::new(cfg, 0);
    }
}
