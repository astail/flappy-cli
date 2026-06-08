//! flappy-core: 純粋なゲームロジック（I/O 依存ゼロ）。
//!
//! `tick()` 駆動の決定論的な状態機械。物理は常に固定 [`DT`] で進む。
//! 物理パート（重力・スクロール・棒生成）まで実装済み。
//! 衝突判定 / スコアは後続 issue で追加する。

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

    pub fn pipes(&self) -> &[Pipe] {
        &self.pipes
    }

    /// フラップ入力。Ready なら Playing 化、いずれにせよ Playing 中は上向き初速を与える。
    /// GameOver では何もしない（restart は別 API、#8）。
    pub fn flap(&mut self) {
        if self.phase == Phase::Ready {
            self.phase = Phase::Playing;
        }
        if self.phase == Phase::Playing {
            self.bird_vy = self.cfg.flap_impulse;
        }
    }

    /// 物理を固定 [`DT`] ぶん進める（Playing 時のみ）。重力→スクロール→棒生成まで。
    /// 衝突判定・スコアは後続 issue で追加する。
    pub fn tick(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }

        // 1. 重力 + 終端速度クランプ → bird_y 更新。
        self.bird_vy = (self.bird_vy + self.cfg.gravity * DT).min(self.cfg.vy_max);
        self.bird_y += self.bird_vy * DT;

        // 2. 全 pipe を左へスクロール、画面外（x < -1）を除去。
        let dx = self.cfg.scroll_speed * DT;
        for p in &mut self.pipes {
            p.x -= dx;
        }
        self.pipes.retain(|p| p.x >= -1.0);

        // 3. 次の棒までの距離を進め、0 以下なら右端に新 pipe を生成。
        //    剰余を保持して spacing が drift しないよう `+=` する。
        self.dist_to_next -= dx;
        if self.dist_to_next <= 0.0 {
            let gap_top = self
                .rng
                .gen_range_inclusive(1, self.cfg.rows - 1 - self.cfg.pipe_gap);
            self.pipes.push(Pipe {
                x: self.cfg.cols as f32,
                gap_top,
                passed: false,
            });
            self.dist_to_next += self.cfg.pipe_spacing;
        }
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

    #[test]
    fn flap_starts_game_and_sets_upward_velocity() {
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        assert_eq!(g.phase(), Phase::Playing);
        assert!(
            g.bird_vy < 0.0,
            "flap should give upward (negative) velocity"
        );
    }

    #[test]
    fn tick_does_nothing_until_playing() {
        let mut g = Game::new(Config::default(), 1);
        let y0 = g.bird_y;
        let x0 = g.pipes()[0].x;
        g.tick(); // Ready のまま
        assert_eq!(g.phase(), Phase::Ready);
        assert_eq!(g.bird_y, y0);
        assert_eq!(g.pipes()[0].x, x0);
    }

    #[test]
    fn gravity_pulls_bird_down_over_time() {
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        let y_start = g.bird_y;
        // 2 秒ぶん（120 tick）回すと、初速の上昇を打ち消して開始位置より下（y 増加）へ。
        for _ in 0..120 {
            g.tick();
        }
        assert!(
            g.bird_y > y_start,
            "gravity should pull the bird below start: {} !> {}",
            g.bird_y,
            y_start
        );
    }

    #[test]
    fn pipes_scroll_left() {
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        let x_before = g.pipes()[0].x;
        g.tick();
        assert!(g.pipes()[0].x < x_before);
    }

    #[test]
    fn same_seed_produces_identical_pipe_spawns() {
        let mut a = Game::new(Config::default(), 777);
        let mut b = Game::new(Config::default(), 777);
        a.flap();
        b.flap();
        // 棒が複数本生成されるだけ回す。
        for _ in 0..400 {
            a.tick();
            b.tick();
        }
        let gaps_a: Vec<u16> = a.pipes().iter().map(|p| p.gap_top).collect();
        let gaps_b: Vec<u16> = b.pipes().iter().map(|p| p.gap_top).collect();
        assert!(gaps_a.len() > 1, "expected multiple pipes spawned");
        assert_eq!(gaps_a, gaps_b, "same seed must yield identical gap_top列");
    }
}
