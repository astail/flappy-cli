//! バイナリ全体の統合スモークテスト。`flappy --headless` を実プロセスで起動し、
//! 引数解析 → autopilot → スコア stdout 出力の一連を検証する。

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

/// (b) 実測ゴールデン（非ゼロ）。autopilot が隙間を追従できる seed=3。
#[test]
fn golden_nonzero_score() {
    let s = run_score("3", "600");
    assert!(s > 0, "expected non-zero score, got {s}");
    assert_eq!(s, 4, "golden score regression");
}
