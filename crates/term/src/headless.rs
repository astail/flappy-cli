//! headless モード: TTY 不要・端末ガード非経由で、決定論 autopilot + 固定 DT により
//! N フレーム自動実行し最終スコアを返す。CI のスモークテスト用。

use flappy_core::{Config, Game};

/// `seed` から `frames` フレーム autopilot 実行し最終スコアを返す。
/// 各フレームは autopilot の判断 → 固定 DT の `tick()`。autopilot は core の
/// [`Game::autopilot_step`] が単一ソース（`--auto` / `?auto=1` の対話デモと同一 bot）。
/// `speedup` 有効時は core の `Config::with_speedup`（score 依存の速度上昇）を使う。
pub fn run_headless(seed: u64, frames: u32, speedup: bool) -> u32 {
    let cfg = if speedup {
        Config::default().with_speedup()
    } else {
        Config::default()
    };
    let mut game = Game::new(cfg, seed);
    for _ in 0..frames {
        game.autopilot_step();
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
        assert_eq!(run_headless(3, 600, false), run_headless(3, 600, false));
        assert_eq!(run_headless(1, 600, false), run_headless(1, 600, false));
    }

    #[test]
    fn autopilot_scores_nonzero_golden() {
        // autopilot が隙間を追従できる seed の実測ゴールデン（非ゼロ）。
        // 単純 bang-bang のため隙間が端寄りの seed（例: 1）では早期に死んで 0 になるが、
        // それも決定論的に安定した出力。
        // ゴールデン値の定義はここ 1 箇所のみ（tests/headless.rs は score > 0 までに留める）。
        assert_eq!(run_headless(3, 600, false), 4);
    }

    #[test]
    fn speedup_same_seed_same_score() {
        // speedup も決定論: 同一 (seed, frames) は常に同一スコア。
        assert_eq!(run_headless(3, 1200, true), run_headless(3, 1200, true));
    }

    #[test]
    fn speedup_passes_more_pipes_than_default_golden() {
        // speedup の実測ゴールデン（非ゼロ・決定論回帰）。速度が上がるぶん同じフレーム数で
        // より多くの棒を通過するため、既定モードよりスコアが伸びる。ゴールデン値の定義は
        // ここ 1 箇所のみ（tests/headless.rs は score > 0 までに留める）。
        assert_eq!(
            run_headless(3, 1200, false),
            9,
            "既定モードのゴールデン（基準）"
        );
        assert_eq!(run_headless(3, 1200, true), 16, "speedup のゴールデン");
    }
}
