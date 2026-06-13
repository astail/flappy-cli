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

/// --help / --version は TUI に入らず即終了し、それぞれ usage / バージョンを出力する。
#[test]
fn help_and_version_print_and_exit() {
    for (flag, expect) in [
        ("--help", "USAGE".to_string()),
        ("-h", "USAGE".to_string()),
        ("--version", format!("flappy {}", flappy_core::VERSION)),
        ("-V", format!("flappy {}", flappy_core::VERSION)),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_flappy"))
            .arg(flag)
            .output()
            .expect("failed to run flappy");
        assert!(out.status.success(), "{flag} exited with {:?}", out.status);
        let stdout = String::from_utf8(out.stdout).expect("utf8 stdout");
        assert!(stdout.contains(&expect), "{flag} stdout: {stdout:?}");
    }
}

/// --seed / --frames の parse 失敗は silent fallback せず非ゼロ終了する。
#[test]
fn invalid_headless_value_exits_nonzero() {
    for args in [
        ["--headless", "--seed", "abc"],
        ["--headless", "--frames", "-1"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_flappy"))
            .args(args)
            .output()
            .expect("failed to run flappy");
        assert!(!out.status.success(), "{args:?} should exit non-zero");
        let stderr = String::from_utf8(out.stderr).expect("utf8 stderr");
        assert!(stderr.contains("不正"), "{args:?} stderr: {stderr:?}");
    }
}
