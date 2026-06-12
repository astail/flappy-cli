//! バイナリ全体の統合スモークテスト。`flappy --headless` を実プロセスで起動し、
//! 引数解析 → autopilot → スコア stdout 出力の一連を検証する。
//!
//! 検証は「同一 seed 2 回走の一致 + score > 0」までに留める。
//! 厳密なゴールデン値は `src/headless.rs` のユニットテスト
//! `autopilot_scores_nonzero_golden` に一本化している。

use std::process::Command;

fn run_score(seed: &str, frames: &str) -> u32 {
    let out = Command::new(env!("CARGO_BIN_EXE_flappy"))
        .args(["--headless", "--seed", seed, "--frames", frames])
        .output()
        .expect("failed to run flappy");
    assert!(out.status.success(), "flappy exited with {:?}", out.status);
    String::from_utf8(out.stdout)
        .expect("utf8 stdout")
        .trim()
        .parse()
        .expect("stdout must be a score integer")
}

/// (a) 決定論: 同一 seed の 2 回走でスコア一致。
#[test]
fn same_seed_is_deterministic() {
    assert_eq!(run_score("3", "600"), run_score("3", "600"));
}

/// (b) 非ゼロスコア。autopilot が隙間を追従できる seed=3。
/// 厳密なゴールデン値の検証は src 側ユニットテストに委ねる。
#[test]
fn nonzero_score() {
    let s = run_score("3", "600");
    assert!(s > 0, "expected non-zero score, got {s}");
}
