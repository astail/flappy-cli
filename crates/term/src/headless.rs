//! headless モード: TTY 不要・端末ガード非経由で、決定論 autopilot + 固定 DT により
//! N フレーム自動実行し最終スコアを返す。CI のスモークテスト用。

use flappy_core::{Config, Game};

/// DESIGN §4 の決定論 autopilot 1 フレーム分。
///
/// 前方（`x >= bird_col`）の未 passed で最寄りの棒、無ければ未 passed の最寄りを狙い、
/// 鳥が隙間中心（`gap_top + pipe_gap/2`）より下なら `flap()`。`x` は f32 のため `partial_cmp`。
pub fn autopilot_step(game: &mut Game) {
    let bird_col = game.config().bird_col;
    let pipe_gap = game.config().pipe_gap;

    // 狙う棒の隙間中心を取り出してから（不変借用を閉じてから）flap する。
    let gap_center = {
        let pipes = game.pipes();
        let target = pipes
            .iter()
            .filter(|p| !p.passed && p.x >= bird_col)
            .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
            .or_else(|| {
                pipes
                    .iter()
                    .filter(|p| !p.passed)
                    .min_by(|a, b| a.x.partial_cmp(&b.x).unwrap())
            });
        target.map(|p| p.gap_top as f32 + pipe_gap as f32 / 2.0)
    };

    if let Some(center) = gap_center {
        if (game.bird_cell().1 as f32) > center {
            game.flap();
        }
    }
}

/// デフォルト Config で `seed` から `frames` フレーム autopilot 実行し最終スコアを返す。
/// 各フレームは autopilot の判断 → 固定 DT の `tick()`。
pub fn run_headless(seed: u64, frames: u32) -> u32 {
    let mut game = Game::new(Config::default(), seed);
    for _ in 0..frames {
        autopilot_step(&mut game);
        game.tick();
    }
    game.score()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_score() {
        // 決定論: 同一 (seed, frames) は常に同一スコア。
        assert_eq!(run_headless(3, 600), run_headless(3, 600));
        assert_eq!(run_headless(1, 600), run_headless(1, 600));
    }

    #[test]
    fn autopilot_scores_nonzero_golden() {
        // autopilot が隙間を追従できる seed の実測ゴールデン（非ゼロ）。
        // 単純 bang-bang のため隙間が端寄りの seed（例: 1）では早期に死んで 0 になるが、
        // それも決定論的に安定した出力。
        // ゴールデン値の定義はここ 1 箇所のみ（tests/headless.rs は score > 0 までに留める）。
        assert_eq!(run_headless(3, 600), 4);
    }
}
