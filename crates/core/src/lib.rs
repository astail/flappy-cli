//! flappy-core: 純粋なゲームロジック（I/O 依存ゼロ）。
//!
//! `tick()` 駆動の決定論的な状態機械。物理は常に固定 [`DT`] で進む。
//! 物理パート（重力・スクロール・棒生成）・衝突判定・スコア加算・restart を実装済み。

mod rng;

use rng::Rng;

/// 物理の固定タイムステップ（秒）。レンダラはアキュムレータで `tick()` 回数を制御する。
pub const DT: f32 = 1.0 / 60.0;

/// 1 フレームで消化する実時間の上限（秒）。描画ヒッチやタブ復帰で溜まった実時間を
/// 一気に tick して即死/カクつく「spiral of death」を防ぐ安全網。[`Accumulator`] が内蔵する。
pub const MAX_FRAME_DT: f32 = 0.10;

/// オートモード（`--auto` / `?auto=1`）で GameOver 後に自動リスタートするまでの待機秒。
/// スコアが目に入るよう少し止める用途。秒数は core が単一ソース（term/web 共有）。
/// 待機自体はレンダラ側の見た目都合のため、決定論を保つ `tick()` には影響しない。
pub const AUTO_RESTART_DELAY_SECS: f32 = 1.0;

/// ビルド時の version（= Cargo.toml の version）。term/web の画面描画（#40）で参照する。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// GameOver 画面のタイトル（#76: term/web が同一ソースを参照し文言ズレを防ぐ）。
pub const GAMEOVER_TITLE: &str = "GAME  OVER";

/// GameOver 画面の retry 案内（SPACE / click は term・web 共通の主操作）。
pub const GAMEOVER_RETRY_HINT: &str = "SPACE / click / r : retry";

/// Ready 画面のタイトル（#107: GameOver 文言と同様 term/web が同一ソースを参照し文言ズレを防ぐ）。
pub const READY_TITLE: &str = "F L A P P Y";

/// Ready 画面の開始案内（行位置は [`layout`] の単一ソース、表現系の微調整は各レンダラ）。
pub const READY_HINT: &str = "──  press SPACE  ──";

/// 画面レイアウトの行/列インデックス（term/web 共有の単一ソース。#141）。
///
/// term は char グリッドの行/列、web は `行 * cell`（px）として参照する。web のテキストは
/// baseline=middle のため `+0.5` セルのオフセットが乗るが、それは canvas 起因の各自事情で
/// 加算するだけで、ここで定義する**行/列インデックス自体は両者で一致**する。
/// version の「最下行右端」は `rows` 依存のため定数化せず、各レンダラが `rows - 1` 等で算出する
/// （共有するのは「右端・最下行に置く」配置規約のみ）。
pub mod layout {
    /// Ready タイトルの行。
    pub const READY_TITLE_ROW: u16 = 3;
    /// Ready 開始案内の行。
    pub const READY_HINT_ROW: u16 = 8;
    /// GameOver 罫線ボックスの上端行。
    pub const GAMEOVER_BOX_TOP: u16 = 2;
    /// GameOver 罫線ボックスの高さ（行）。内部 4 行（title/score/retry/quit）＋上下罫線。
    pub const GAMEOVER_BOX_HEIGHT: u16 = 6;
    /// HUD（SCORE / BEST）の行（最上行に天井ラインと重ねる）。
    pub const HUD_ROW: u16 = 0;
    /// HUD の SCORE 左端の列。
    pub const HUD_SCORE_COL: u16 = 1;
}

/// 固定タイムステップ（[`DT`]）のアキュムレータ。実経過時間を渡すと、消化すべき `tick()` 数を返す。
/// [`MAX_FRAME_DT`] クランプを内蔵し、`DT` 未満の端数は内部に保持して次フレームへ繰り越す
/// （描画頻度に依存しない決定論的な物理進行）。term/web はこの「クランプ＋固定ステップ消化」を共有し、
/// 実時間の取得源（`Instant` / RAF timestamp）のみ各レンダラが持つ（#139）。
#[derive(Debug, Default)]
pub struct Accumulator {
    acc: f32,
}

impl Accumulator {
    /// 空（未消化時間 0）の状態で生成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 実経過時間 `real_dt`（秒）を [`MAX_FRAME_DT`] でクランプして加算し、消化できる固定ステップ数を
    /// 返す。返した分は内部から差し引き、`DT` 未満の端数は保持して次回 `advance` に繰り越す。
    pub fn advance(&mut self, real_dt: f32) -> u32 {
        self.acc += real_dt.min(MAX_FRAME_DT);
        let mut ticks = 0;
        while self.acc >= DT {
            self.acc -= DT;
            ticks += 1;
        }
        ticks
    }
}

/// チューニング値の集約。デフォルトは DESIGN §7 の初期値。
pub struct Config {
    pub cols: u16,
    pub rows: u16,
    pub bird_col: f32,
    /// 重力（行/秒^2）
    pub gravity: f32,
    /// フラップの上向き初速（負値）
    pub flap_impulse: f32,
    /// スクロール速度（列/秒）。スピードアップ時の初速（base）でもある。
    pub scroll_speed: f32,
    /// スコア 1 本あたりのスクロール速度上昇量（列/秒）。既定 0.0 = 上昇なし（後方互換）。
    pub scroll_speed_step: f32,
    /// スクロール速度の上限（列/秒）。既定は `scroll_speed`（= 上昇なし）。
    pub scroll_speed_max: f32,
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
            // 既定は実質オフ: step=0・max=scroll_speed なので実効速度は常に scroll_speed（一定）。
            scroll_speed_step: 0.0,
            scroll_speed_max: 12.0,
            pipe_gap: 6,
            pipe_spacing: 22.0,
            vy_max: 30.0,
        }
    }
}

impl Config {
    /// スピードアップモードの Config を返す（`--speedup` / `?speedup=1`）。
    /// score に応じてスクロール速度が `scroll_speed_step` ずつ上がり `scroll_speed_max` で
    /// クランプする。term/web の drift を防ぐため step / cap の数値はここが唯一のソース。
    /// cap は不変条件（`scroll_speed_max × DT < 1.0` → < 60 列/秒）の範囲に収め、衝突は
    /// 従来の離散等値判定のまま据え置く（DESIGN §7）。
    pub fn with_speedup(self) -> Self {
        Self {
            scroll_speed_step: 1.5,
            scroll_speed_max: 30.0,
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Playing,
    GameOver,
}

/// 主操作（Space / click / tap）が phase に応じて落とす効果。
/// GameOver はリスタート、Ready/Playing はフラップ（Ready は初回フラップで Playing 化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryAction {
    Flap,
    Restart,
}

/// 主操作を受けたとき phase から効果を決める純粋関数（#137: 判定の単一ソース）。
///
/// 入力の「分類」（どのキー/イベントを主操作とみなすか）は各レンダラの責務で、
/// core が持つのは「主操作を受けた後、phase を見て flap/restart のどちらに落とすか」の
/// 1 点のみ。term の `input::route`（`Input::Primary` 分岐）と web の `apply_primary` が
/// 両方これを経由し、二重実装による drift を防ぐ。
pub fn primary_action(phase: Phase) -> PrimaryAction {
    if phase == Phase::GameOver {
        PrimaryAction::Restart
    } else {
        PrimaryAction::Flap
    }
}

pub struct Pipe {
    pub x: f32,
    pub gap_top: u16,
    pub passed: bool,
    /// この棒がコース（`with_course` の gap_top 列）の何番目から生成されたか。
    /// レンダラが「どの行の文字でこの棒を描くか」を引くのに使う（term `--cmd`）。
    /// 乱数モード（course 空）では常に 0。
    pub course_idx: usize,
}

/// 棒が指定 `row`（行）を塞ぐか。**描画（棒セル）と衝突判定で共有する単一定義**。
/// これにより「隙間を通ったのに死ぬ」描画/判定の乖離を防ぐ。
///
/// 棒セル（塞ぐ範囲）= `1 ≤ row < gap_top` ∪ `gap_top + pipe_gap ≤ row ≤ rows-2`。
/// 隙間（通れる範囲）は `gap_top ≤ row < gap_top + pipe_gap`。
pub fn pipe_blocks_row(gap_top: u16, pipe_gap: u16, rows: u16, row: i32) -> bool {
    let top = gap_top as i32;
    let gap_bottom = top + pipe_gap as i32; // 隙間直下の最初の行
    let ground = rows as i32 - 2; // 鳥が乗れる最下行
    (1 <= row && row < top) || (gap_bottom <= row && row <= ground)
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
    score: u32,
    best: u32,
    /// 棒の gap_top 列（term `--cmd`）。空なら rng で抽選、非空なら順に（尽きたら先頭へ）消費する。
    course: Vec<u16>,
    /// course の次に読む位置（reset で 0 に戻り、毎プレイ先頭から再生する）。
    course_pos: usize,
}

impl Game {
    pub fn new(cfg: Config, seed: u64) -> Self {
        // course 空 = 従来の rng 抽選経路。
        Self::with_course(cfg, seed, Vec::new())
    }

    /// 棒の `gap_top` 列（course）を外から与えて Game を作る（term の `--cmd` 用）。
    /// `course` が非空なら `spawn_pipe` は rng でなくこの列を順に（尽きたら先頭へループ）消費する。
    /// 各値は `[1, rows-1-pipe_gap]` にクランプされるため範囲外を渡しても安全。
    /// `course` が空なら [`Game::new`] と完全に同一（rng 抽選）。
    pub fn with_course(cfg: Config, seed: u64, course: Vec<u16>) -> Self {
        // 1 フレーム 1 セル以内を保証する不変条件（スイープ判定を不要にする）。
        // scroll_speed_max も含めることで、speedup の ramp が上限に達しても 1 セル/フレームを
        // 割る保証になる（cap ≥ 60 の Config は構築時に panic で弾く）。
        assert!(
            cfg.scroll_speed * DT < 1.0 && cfg.scroll_speed_max * DT < 1.0 && cfg.vy_max * DT < 1.0,
            "config violates per-frame 1-cell invariant"
        );
        // gap_top の抽選範囲 [1, rows - 1 - pipe_gap] が空にならないことを保証する。
        // これが破れると rows - 1 - pipe_gap がアンダーフローし、棒が画面外に配置される。
        assert!(
            cfg.pipe_gap + 2 <= cfg.rows,
            "config violates pipe_gap + 2 <= rows (gap_top range would be empty)"
        );

        let mut game = Self {
            cfg,
            rng: Rng::new(seed),
            phase: Phase::Ready,
            bird_y: 0.0,
            bird_vy: 0.0,
            pipes: Vec::new(),
            dist_to_next: 0.0,
            score: 0,
            best: 0,
            course,
            course_pos: 0,
        };
        game.reset_play_state();
        game
    }

    /// gap_top を決め、右端（x = cols）に未通過の Pipe を 1 本生成する。
    /// gap_top の範囲 `[1, rows - 1 - pipe_gap]` はここが単一ソース（new / tick / restart 共通）。
    /// course 非空ならその列を順に消費（尽きたら先頭へループ）し範囲内へクランプ、空なら rng で抽選。
    fn spawn_pipe(&mut self) -> Pipe {
        let hi = self.cfg.rows - 1 - self.cfg.pipe_gap;
        let (gap_top, course_idx) = if self.course.is_empty() {
            (self.rng.gen_range_inclusive(1, hi), 0)
        } else {
            let idx = self.course_pos % self.course.len();
            self.course_pos += 1;
            (self.course[idx].clamp(1, hi), idx)
        };
        Pipe {
            x: self.cfg.cols as f32,
            gap_top,
            passed: false,
            course_idx,
        }
    }

    /// プレイ開始状態へ初期化する（new / restart 共通）。`best` と rng ストリームは touch しない。
    /// 鳥を画面中央付近へ、最初の棒を右端に 1 本（鳥に届くまで約 1 画面ぶんの助走で開始即死を防ぐ）、
    /// `dist_to_next` を spacing に、phase を Ready、score を 0 にする。
    /// course モードでは `course_pos` を 0 に戻し、リスタートのたびに同じコースを先頭から再生する。
    fn reset_play_state(&mut self) {
        self.bird_y = self.cfg.rows as f32 / 2.0;
        self.bird_vy = 0.0;
        self.course_pos = 0;
        let pipe = self.spawn_pipe();
        self.pipes = vec![pipe];
        self.dist_to_next = self.cfg.pipe_spacing;
        self.phase = Phase::Ready;
        self.score = 0;
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn pipes(&self) -> &[Pipe] {
        &self.pipes
    }

    /// 描画に必要なグリッド寸法・パラメータ（cols/rows/pipe_gap 等）を参照する。
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// 現在スコア。加点は tick 内でのみ行われ、外部からは読み取り専用。
    pub fn score(&self) -> u32 {
        self.score
    }

    /// 最高スコア。tick の加点に追従して更新され、外部からは読み取り専用。
    pub fn best(&self) -> u32 {
        self.best
    }

    /// 鳥の離散行（`bird_y.round()`）。衝突判定と描画で同一の丸めを使うための単一ソース。
    fn bird_row(&self) -> i32 {
        self.bird_y.round() as i32
    }

    /// 現在のスコアに応じた実効スクロール速度（列/秒）。`scroll_speed + step * score` を
    /// `scroll_speed_max` でクランプする。既定 Config（step=0・max=scroll_speed）では常に
    /// `scroll_speed` で一定（後方互換）。決定論ガードレール遵守: f32 の加算・乗算・`min` と
    /// `as f32` キャストのみ（`mul_add`・transcendental は使わない）。
    fn current_scroll_speed(&self) -> f32 {
        (self.cfg.scroll_speed + self.cfg.scroll_speed_step * self.score as f32)
            .min(self.cfg.scroll_speed_max)
    }

    /// 鳥の衝突セル `(col, row)`。衝突判定と同一の丸め（row は `max(0)` で天井行 0 まで許す）。
    /// 描画は天井ライン/HUD 帯（row 0）を潰さないため [`bird_display_cell`](Self::bird_display_cell)
    /// を使う（用途で使い分け。#138）。
    pub fn bird_cell(&self) -> (u16, u16) {
        let col = (self.cfg.bird_col as i32).max(0) as u16;
        let row = self.bird_row().max(0) as u16;
        (col, row)
    }

    /// 鳥の描画セル `(col, row)`。衝突用 [`bird_cell`](Self::bird_cell) の row を 1 以上に
    /// クランプする。row 0 は天井ライン / HUD 帯であり、ここに鳥（● / ✕ / web の塗り円）を
    /// 描くと帯を潰すため、天井死などで row が 0 に来てもプレイエリア最上行（row 1）へ寄せる
    /// （#112 系のズレ防止）。term/web の鳥描画はすべてこれを経由する（#138: 3 箇所に重複していた
    /// `row.max(1)` の単一ソース）。衝突判定は row 0 を含める必要があるため `bird_cell` と使い分ける。
    pub fn bird_display_cell(&self) -> (u16, u16) {
        let (col, row) = self.bird_cell();
        (col, row.max(1))
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

    /// 物理を固定 [`DT`] ぶん進める（Playing 時のみ）。
    /// 重力→スクロール→棒生成→衝突判定→スコア加算。
    pub fn tick(&mut self) {
        if self.phase != Phase::Playing {
            return;
        }

        // 1. 重力 + 終端速度クランプ → bird_y 更新。
        self.bird_vy = (self.bird_vy + self.cfg.gravity * DT).min(self.cfg.vy_max);
        self.bird_y += self.bird_vy * DT;

        // 2. 全 pipe を左へスクロール、画面外（x < -1）を除去。
        //    dx は score 依存の実効速度（speedup 無効の既定では scroll_speed で一定）。
        //    pipe の左移動と dist_to_next の減算で使い回すため、spawn 間隔も自動で追従する。
        let dx = self.current_scroll_speed() * DT;
        for p in &mut self.pipes {
            p.x -= dx;
        }
        self.pipes.retain(|p| p.x >= -1.0);

        // 3. 次の棒までの距離を進め、0 以下なら右端に新 pipe を生成。
        //    剰余を保持して spacing が drift しないよう `+=` する。
        self.dist_to_next -= dx;
        if self.dist_to_next <= 0.0 {
            let pipe = self.spawn_pipe();
            self.pipes.push(pipe);
            self.dist_to_next += self.cfg.pipe_spacing;
        }

        // 4. 衝突判定（先に評価。当たれば GameOver、加点しない＝スコアは #8）。
        let bird_row = self.bird_row();
        let bird_c = self.cfg.bird_col as i32;
        let rows = self.cfg.rows;
        let hit_bounds = bird_row < 1 || bird_row >= rows as i32 - 1;
        // 鳥列に重なる棒があり、その行が隙間の外（= 棒セル）なら衝突。
        // 境界（row<1 / row>rows-2）は上の hit_bounds で先に弾くため、ここは
        // pipe_blocks_row の定義と一致する。
        let hit_pipe = self.pipes.iter().any(|p| {
            p.x.round() as i32 == bird_c
                && pipe_blocks_row(p.gap_top, self.cfg.pipe_gap, rows, bird_row)
        });
        if hit_bounds || hit_pipe {
            self.phase = Phase::GameOver;
            return;
        }

        // 5. スコア（衝突しなかった場合のみ）。鳥を完全に通り抜けた（pipe_col < bird_c）
        //    未 passed の棒を passed 化し加点。best も更新。
        let mut gained = 0u32;
        for p in &mut self.pipes {
            if !p.passed && (p.x.round() as i32) < bird_c {
                p.passed = true;
                gained += 1;
            }
        }
        if gained > 0 {
            self.score += gained;
            if self.score > self.best {
                self.best = self.score;
            }
        }
    }

    /// best を保持してゲームを初期化する。rng は再シードせず既存の決定論ストリームを
    /// そのまま継続するため、リスタートのたびに棒配置は変わる（同一プレイの繰り返しを避ける）。
    pub fn restart(&mut self) {
        // new と同じ初期化。reset_play_state は best も rng ストリームも触らないため、
        // best は維持され、棒配置はリスタートのたびに変わる（同一プレイの繰り返しを避ける）。
        self.reset_play_state();
    }

    /// 決定論 autopilot の 1 ステップ（DESIGN §4）。`--headless` ベンチと `--auto` / `?auto=1` の
    /// 対話デモを同一 bot で駆動する単一ソース（公開 API のみ使用）。
    ///
    /// 前方（`x >= bird_col`）の未 passed で最寄りの棒、無ければ未 passed の最寄りを狙い、
    /// 鳥が隙間中心（`gap_top + pipe_gap/2`）より下なら [`flap`](Self::flap)。`x` は f32 のため
    /// `partial_cmp`。Ready なら flap が Playing 化し、GameOver では flap が no-op（呼んでも安全）。
    pub fn autopilot_step(&mut self) {
        let bird_col = self.cfg.bird_col;
        let pipe_gap = self.cfg.pipe_gap;

        // 狙う棒の隙間中心を取り出してから（不変借用を閉じてから）flap する。
        let gap_center = {
            let target = self
                .pipes
                .iter()
                .filter(|p| !p.passed && p.x >= bird_col)
                .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
                .or_else(|| {
                    self.pipes
                        .iter()
                        .filter(|p| !p.passed)
                        .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
                });
            target.map(|p| p.gap_top as f32 + pipe_gap as f32 / 2.0)
        };

        if let Some(center) = gap_center {
            if (self.bird_cell().1 as f32) > center {
                self.flap();
            }
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
        assert_eq!(g.score(), 0);
        assert_eq!(g.best(), 0);
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
    #[should_panic(expected = "pipe_gap + 2 <= rows")]
    fn new_panics_when_pipe_gap_too_large() {
        // pipe_gap >= rows - 1 だと gap_top の範囲 [1, rows-1-pipe_gap] が空になり
        // rows - 1 - pipe_gap がアンダーフローする。new で弾く。
        let cfg = Config {
            rows: 24,
            pipe_gap: 23, // pipe_gap + 2 = 25 > rows(24)
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
    fn primary_action_flaps_unless_gameover() {
        // 主操作の効果ルーティングの単一ソース（#137）。term/web 双方がこれを経由する。
        assert_eq!(primary_action(Phase::Ready), PrimaryAction::Flap);
        assert_eq!(primary_action(Phase::Playing), PrimaryAction::Flap);
        assert_eq!(primary_action(Phase::GameOver), PrimaryAction::Restart);
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
        // #7 で tick に衝突が入ったため、フラップしないと鳥が早期に GameOver して
        // 棒が増えない。両ゲームに同一の hover ポリシーを適用して生かしたまま
        // 複数の棒を生成し、決定論（gap_top 列の完全一致）を検証する。
        let center = Config::default().rows / 2;
        let mut a = Game::new(Config::default(), 777);
        let mut b = Game::new(Config::default(), 777);
        for _ in 0..400 {
            for g in [&mut a, &mut b] {
                // row >= center で flap（初手で Ready→Playing 化し、以降は中央付近で hover）。
                if g.bird_cell().1 >= center {
                    g.flap();
                }
                g.tick();
            }
        }
        let gaps_a: Vec<u16> = a.pipes().iter().map(|p| p.gap_top).collect();
        let gaps_b: Vec<u16> = b.pipes().iter().map(|p| p.gap_top).collect();
        assert!(gaps_a.len() > 1, "expected multiple pipes alive");
        assert_eq!(gaps_a, gaps_b, "same seed must yield identical gap_top列");
    }

    #[test]
    fn pipe_blocks_row_matches_design_definition() {
        let (gap_top, pipe_gap, rows) = (10u16, 6u16, 24u16);
        // 隙間 [10, 16) は通れる。
        for row in 10..16 {
            assert!(
                !pipe_blocks_row(gap_top, pipe_gap, rows, row),
                "row {row} should be open (gap)"
            );
        }
        // 隙間より上 [1, 10) は塞ぐ。
        for row in 1..10 {
            assert!(pipe_blocks_row(gap_top, pipe_gap, rows, row), "row {row}");
        }
        // 隙間より下 [16, 22=rows-2] は塞ぐ。
        for row in 16..=22 {
            assert!(pipe_blocks_row(gap_top, pipe_gap, rows, row), "row {row}");
        }
        // 天井行 0・地面行 rows-1 は棒セルではない（境界判定に委ねる）。
        assert!(!pipe_blocks_row(gap_top, pipe_gap, rows, 0));
        assert!(!pipe_blocks_row(gap_top, pipe_gap, rows, 23));
    }

    #[test]
    fn falling_into_ground_triggers_gameover() {
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        // フラップせず落下し続ければ地面に到達して GameOver。
        for _ in 0..300 {
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        let (_, row) = g.bird_cell();
        assert!(
            row >= Config::default().rows - 1,
            "should die at/below ground line, row={row}"
        );
    }

    #[test]
    fn flapping_into_ceiling_triggers_gameover() {
        let mut g = Game::new(Config::default(), 1);
        // 毎 tick フラップし続ければ上昇して天井に到達。
        for _ in 0..300 {
            g.flap();
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        let (_, row) = g.bird_cell();
        assert!(row < 1, "should die at ceiling, row={row}");
    }

    #[test]
    fn bird_display_cell_clamps_row_to_play_area_top() {
        // 天井死では衝突用 row が 0（天井ライン/HUD 帯）に来るが、描画用は row 1 へ寄せる（#138）。
        let mut g = Game::new(Config::default(), 1);
        for _ in 0..300 {
            g.flap();
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        assert_eq!(g.bird_cell().1, 0, "天井死は衝突用 row が 0");
        assert_eq!(g.bird_display_cell().1, 1, "描画用 row は 1 にクランプ");
        assert_eq!(g.bird_display_cell().0, g.bird_cell().0, "col は据え置き");
    }

    #[test]
    fn bird_display_cell_equals_bird_cell_when_row_positive() {
        // 通常飛行（row >= 1）では描画用と衝突用は一致する（クランプが効かない）。
        let g = Game::new(Config::default(), 1);
        assert!(g.bird_cell().1 >= 1, "Ready は中央付近で row >= 1");
        assert_eq!(g.bird_display_cell(), g.bird_cell());
    }

    #[test]
    fn bird_hits_pipe_outside_gap_triggers_gameover() {
        // 鳥を右端寄りに置き最初の棒がすぐ到達するようにし、隙間が鳥の行を
        // 外す seed を探す（決定論・有限）。境界でない GameOver = 棒衝突。
        let rows = Config::default().rows;
        for seed in 0..200u64 {
            let mut g = Game::new(
                Config {
                    bird_col: 60.0,
                    ..Config::default()
                },
                seed,
            );
            g.flap();
            for _ in 0..120 {
                g.tick();
                if g.phase() == Phase::GameOver {
                    let (_, row) = g.bird_cell();
                    if row >= 1 && row <= rows - 2 {
                        return; // 境界でない GameOver = 棒衝突を再現できた
                    }
                    break; // この seed は境界衝突。次の seed へ。
                }
            }
        }
        panic!("no seed produced a pipe collision within bounds");
    }

    #[test]
    fn passing_one_pipe_scores_exactly_one() {
        // hover で生き延びて最初の棒を通過する seed を探し、最初の加点が
        // ちょうど 1（棒 1 本ずつ）で best も 1 になることを検証する。
        let center = Config::default().rows / 2;
        for seed in 0..200u64 {
            let mut g = Game::new(Config::default(), seed);
            for _ in 0..1000 {
                if g.bird_cell().1 >= center {
                    g.flap();
                }
                g.tick();
                if g.score() > 0 {
                    assert_eq!(g.score(), 1, "first scoring event must be exactly 1");
                    assert_eq!(g.best(), 1, "best must track the first score");
                    return;
                }
                if g.phase() == Phase::GameOver {
                    break;
                }
            }
        }
        panic!("no seed let the bird pass a pipe");
    }

    #[test]
    fn scripted_hover_accumulates_score_with_best_tracking() {
        // スクリプト化した hover フラップ列で回し、複数の棒を通過してスコアが
        // 積み上がること・score が単調非減少で best が追従することを検証する。
        let center = Config::default().rows / 2;
        for seed in 0..200u64 {
            let mut g = Game::new(Config::default(), seed);
            let mut prev = 0;
            for _ in 0..3000 {
                if g.bird_cell().1 >= center {
                    g.flap();
                }
                g.tick();
                assert!(g.score() >= prev, "score must be monotonic non-decreasing");
                assert_eq!(g.best(), g.score(), "best must track score while climbing");
                prev = g.score();
                if g.phase() == Phase::GameOver {
                    break;
                }
            }
            if g.score() >= 2 {
                return; // 複数棒通過でスコア加算を確認
            }
        }
        panic!("no seed accumulated score >= 2 under hover");
    }

    #[test]
    fn restart_keeps_best_and_resets_state() {
        let cfg = Config::default();
        let (cols, bird_col, center) = (cfg.cols, cfg.bird_col, cfg.rows / 2);
        let spacing = cfg.pipe_spacing;
        // スコアを稼げる seed を探し、restart 後の状態を検証する。
        for seed in 0..200u64 {
            let mut g = Game::new(Config::default(), seed);
            for _ in 0..3000 {
                if g.bird_cell().1 >= center {
                    g.flap();
                }
                g.tick();
                if g.phase() == Phase::GameOver {
                    break;
                }
            }
            if g.score() >= 1 {
                let best = g.best();
                g.restart();
                assert_eq!(g.phase(), Phase::Ready);
                assert_eq!(g.score(), 0);
                assert_eq!(g.best(), best, "best must be preserved across restart");
                assert_eq!(g.pipes().len(), 1);
                assert_eq!(g.pipes()[0].x, cols as f32);
                assert_eq!(g.dist_to_next, spacing);
                assert_eq!(g.bird_cell(), (bird_col as u16, center));
                return;
            }
        }
        panic!("no seed produced a score to test restart");
    }

    #[test]
    fn restart_advances_rng_stream() {
        // restart() は rng を再シードせず既存ストリームを継続する（lib.rs の doc コメント）。
        // new / restart は spawn_pipe() で 1 draw ずつ消費するため、同じ seed の Rng を
        // 独立に進めた gap_top 列と一致するはず。再シード実装に変えると 2 本目以降がずれて落ちる。
        let cfg = Config::default();
        let (lo, hi) = (1, cfg.rows - 1 - cfg.pipe_gap);
        let seed = 42u64;

        let mut rng = Rng::new(seed);
        let expected: Vec<u16> = (0..5).map(|_| rng.gen_range_inclusive(lo, hi)).collect();
        // 全 draw が偶然同値だと再シードを検出できないため、seed 選定をガードする。
        assert!(
            expected.windows(2).any(|w| w[0] != w[1]),
            "degenerate seed: all draws equal, pick another seed"
        );

        let mut g = Game::new(Config::default(), seed);
        let mut actual = vec![g.pipes()[0].gap_top];
        for _ in 1..expected.len() {
            g.restart();
            actual.push(g.pipes()[0].gap_top);
        }
        assert_eq!(
            actual, expected,
            "restart must continue the rng stream, not re-seed"
        );
    }

    #[test]
    fn with_course_cycles_gap_tops_and_tags_index() {
        // course を順に消費し、尽きたら先頭へループする。各 Pipe.course_idx も対応する。
        let course = vec![3u16, 9, 15];
        let mut g = Game::with_course(Config::default(), 1, course);
        // reset_play_state が 1 本目（course[0]）を生成済み。
        let mut got = vec![(g.pipes()[0].gap_top, g.pipes()[0].course_idx)];
        for _ in 0..5 {
            let p = g.spawn_pipe();
            got.push((p.gap_top, p.course_idx));
        }
        assert_eq!(
            got,
            vec![(3, 0), (9, 1), (15, 2), (3, 0), (9, 1), (15, 2)],
            "course must cycle in order with matching course_idx"
        );
    }

    #[test]
    fn with_course_clamps_gap_top_into_range() {
        // 範囲外の gap_top を渡してもクランプされ、棒が画面内（[1, rows-1-pipe_gap]）に収まる。
        let cfg = Config::default();
        let hi = cfg.rows - 1 - cfg.pipe_gap; // 17
        let mut g = Game::with_course(Config::default(), 1, vec![0u16, 99, 5]);
        assert_eq!(g.pipes()[0].gap_top, 1, "0 must clamp up to 1");
        assert_eq!(g.spawn_pipe().gap_top, hi, "99 must clamp down to hi");
        assert_eq!(g.spawn_pipe().gap_top, 5, "in-range value passes through");
    }

    #[test]
    fn restart_replays_course_from_top() {
        // restart で course_pos が 0 に戻り、1 本目が course[0] に戻る（毎プレイ同じコース）。
        let mut g = Game::with_course(Config::default(), 1, vec![7u16, 2, 14]);
        assert_eq!(g.pipes()[0].gap_top, 7);
        // 何本か spawn して course_pos を進めてから restart。
        let _ = g.spawn_pipe();
        let _ = g.spawn_pipe();
        g.restart();
        assert_eq!(g.pipes().len(), 1);
        assert_eq!(g.pipes()[0].gap_top, 7, "restart replays course[0]");
        assert_eq!(g.pipes()[0].course_idx, 0);
    }

    #[test]
    fn empty_course_uses_rng_path() {
        // course 空 = 従来の rng 経路。同一 seed で gap_top 列が一致し course_idx は 0 のまま。
        let a = Game::with_course(Config::default(), 55, Vec::new());
        let b = Game::new(Config::default(), 55);
        assert_eq!(a.pipes()[0].gap_top, b.pipes()[0].gap_top);
        assert_eq!(a.pipes()[0].course_idx, 0);
    }

    #[test]
    fn default_config_scroll_speed_is_constant() {
        // 既定 Config は step=0・max=scroll_speed なので score に依らず実効速度が一定（回帰）。
        let base = Config::default().scroll_speed;
        let mut g = Game::new(Config::default(), 1);
        for score in [0u32, 1, 5, 20, 1000] {
            g.score = score;
            assert_eq!(
                g.current_scroll_speed(),
                base,
                "default config must not ramp (score={score})"
            );
        }
    }

    #[test]
    fn speedup_scroll_speed_ramps_and_clamps() {
        let cfg = Config::default().with_speedup();
        let (base, step, cap) = (
            cfg.scroll_speed,
            cfg.scroll_speed_step,
            cfg.scroll_speed_max,
        );
        assert!(base < cap, "ramp が意味を持つには base < cap が必要");
        let mut g = Game::new(Config::default().with_speedup(), 1);
        g.score = 0;
        assert_eq!(g.current_scroll_speed(), base, "score 0 は base と一致");
        g.score = 1;
        assert_eq!(
            g.current_scroll_speed(),
            base + step,
            "score 1 本ぶん上がる"
        );
        g.score = 100_000;
        assert_eq!(g.current_scroll_speed(), cap, "cap でクランプする");
    }

    #[test]
    #[should_panic(expected = "invariant")]
    fn new_panics_when_scroll_speed_max_violates_invariant() {
        // cap ≥ 60（max * DT ≥ 1.0）の Config は構築時に弾く（離散等値判定の取りこぼし防止）。
        let cfg = Config {
            scroll_speed_max: 72.0, // 72 * (1/60) = 1.2 >= 1.0
            ..Config::default()
        };
        let _ = Game::new(cfg, 0);
    }

    #[test]
    fn with_speedup_satisfies_invariant() {
        // with_speedup（cap=30 < 60）は不変条件を満たし panic しない。
        let cfg = Config::default().with_speedup();
        assert!(cfg.scroll_speed_max * DT < 1.0);
        let _ = Game::new(Config::default().with_speedup(), 0);
    }

    #[test]
    fn speedup_is_deterministic() {
        // speedup でも同一 seed・同一スクリプト入力なら gap_top 列と score が完全一致（決定論）。
        let center = Config::default().rows / 2;
        let mut a = Game::new(Config::default().with_speedup(), 777);
        let mut b = Game::new(Config::default().with_speedup(), 777);
        for _ in 0..400 {
            for g in [&mut a, &mut b] {
                if g.bird_cell().1 >= center {
                    g.flap();
                }
                g.tick();
            }
        }
        let gaps_a: Vec<u16> = a.pipes().iter().map(|p| p.gap_top).collect();
        let gaps_b: Vec<u16> = b.pipes().iter().map(|p| p.gap_top).collect();
        assert_eq!(a.score(), b.score(), "speedup の score は決定論的");
        assert_eq!(gaps_a, gaps_b, "speedup の gap_top列は決定論的");
    }

    #[test]
    fn restart_resets_speedup_to_base() {
        // restart は score を 0 に戻すので実効速度も base に戻る（追加の状態リセット不要）。
        let base = Config::default().with_speedup().scroll_speed;
        let mut g = Game::new(Config::default().with_speedup(), 1);
        g.score = 8;
        assert!(g.current_scroll_speed() > base, "score>0 で base より速い");
        g.restart();
        assert_eq!(g.score(), 0);
        assert_eq!(g.current_scroll_speed(), base, "restart で base に戻る");
    }

    #[test]
    fn accumulator_consumes_whole_steps() {
        // DT 3 つぶんの実時間 → ちょうど 3 tick（端数はほぼ 0）。
        let mut a = Accumulator::new();
        assert_eq!(a.advance(DT * 3.0), 3);
    }

    #[test]
    fn accumulator_clamps_large_dt() {
        // MAX_FRAME_DT を超える実時間は spiral of death 防止でクランプされ、
        // 0.10s 相当（≈ MAX_FRAME_DT / DT = 6 tick）を超える tick は返さない。
        let mut a = Accumulator::new();
        let n = a.advance(1.0); // 1 秒（>> MAX_FRAME_DT）
        let max_ticks = (MAX_FRAME_DT / DT).ceil() as u32;
        assert!(
            n <= max_ticks,
            "clamped tick count {n} must be <= {max_ticks}"
        );
        assert!(
            n >= max_ticks - 1,
            "MAX_FRAME_DT worth should still yield ~{max_ticks} ticks, got {n}"
        );
    }

    #[test]
    fn accumulator_carries_remainder() {
        // 端数を捨てず次回へ繰り越す: DT*1.5 を 2 回 → 合計 3 tick
        // （繰り越さなければ各回 1 tick で合計 2 にしかならない）。
        let mut a = Accumulator::new();
        let total = a.advance(DT * 1.5) + a.advance(DT * 1.5);
        assert_eq!(total, 3);
    }

    #[test]
    fn autopilot_flaps_when_below_gap_center() {
        // 鳥を狙う棒の隙間中心より十分下に置くと、autopilot_step は flap して
        // 上向き初速（flap_impulse）を与える。Ready→Playing 化も flap が担う。
        let mut g = Game::new(Config::default(), 1);
        g.flap(); // Playing 化
        let center = g.pipes()[0].gap_top as f32 + g.cfg.pipe_gap as f32 / 2.0;
        g.bird_y = center + 5.0; // 中心より下
        g.bird_vy = 3.0; // 落下中
        g.autopilot_step();
        assert_eq!(
            g.bird_vy, g.cfg.flap_impulse,
            "中心より下なら flap して上向き初速になる"
        );
    }

    #[test]
    fn autopilot_demo_loop_recovers_after_death() {
        // レンダラの「眺める用ループ」を core で再現し、死亡から確実に復帰することを担保する。
        // 自然死は seed 依存（bot が長く生き残る seed もある）ため、まず落下で確定的に死なせる。
        let mut g = Game::new(Config::default(), 1);
        g.flap(); // Playing 化
        for _ in 0..600 {
            g.tick(); // flap せず落下 → 地面で GameOver
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver, "落下で GameOver になる");

        // GameOver なら restart、その後は autopilot_step → tick で進む（= auto モードのループ）。
        g.restart();
        assert_eq!(g.phase(), Phase::Ready);
        assert_eq!(g.score(), 0, "restart で score は 0 に戻る");

        let mut reached_playing_after_restart = false;
        for _ in 0..300 {
            if g.phase() == Phase::GameOver {
                g.restart();
                continue;
            }
            g.autopilot_step();
            g.tick();
            if g.phase() == Phase::Playing {
                reached_playing_after_restart = true;
            }
        }
        assert!(
            reached_playing_after_restart,
            "restart 後に autopilot が Playing を駆動し、進み続けること"
        );
    }
}
