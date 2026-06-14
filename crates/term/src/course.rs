//! `--cmd "<command>"` モード: コマンド出力を「文字壁」コースへ変換する（term 専用）。
//!
//! コマンドを実行して stdout を行に分割し（→ 各行が 1 本の棒）、各行から隙間の縦位置
//! `gap_top` を決めて core へ渡す列を作る。棒セルの描画（行の文字を縦に敷き詰める）は
//! [`crate::scene`] 側が `Pipe.course_idx` から行を引いて行う。
//!
//! core はシェルコマンドを実行できない web には載らないため、これは term 限定の機能。

use std::io;
use std::process::Command;

use flappy_core::Config;

/// `cmd` を `sh -c` で実行し、stdout を空でない行の列にして返す。
/// 取り込むのはユーザーが明示的に渡した自分のコマンドの出力のみ。
/// spawn 自体に失敗した場合のみ `Err`（出力 0 行などは呼び出し側が判定する）。
pub fn run_command(cmd: &str) -> io::Result<Vec<String>> {
    let output = Command::new("sh").arg("-c").arg(cmd).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    Ok(lines)
}

/// 1 行から隙間の縦位置 `gap_top` を `[1, rows-1-pipe_gap]` の範囲で決める。
///
/// FNV-1a で行内容をハッシュして散らす（同じ出力 → 同じコース・行ごとにバラつく）。
/// `ls -la` は行幅がほぼ均一なので「行長」だと平坦になるため内容ハッシュを採用。
/// **行長で写像したい場合はこの関数 1 つを差し替えればよい**（例: `1 + (len % hi)`）。
pub fn gap_top_for(line: &str, rows: u16, pipe_gap: u16) -> u16 {
    let hi = (rows - 1 - pipe_gap) as u64; // 取りうる gap_top の本数（[1, hi]）
    1 + (fnv1a(line.as_bytes()) % hi) as u16
}

/// 各行を `gap_top_for` で写像した列（index i = `lines[i]` 由来）。
pub fn build_course(lines: &[String], cfg: &Config) -> Vec<u16> {
    lines
        .iter()
        .map(|line| gap_top_for(line, cfg.rows, cfg.pipe_gap))
        .collect()
}

/// FNV-1a 64bit ハッシュ（依存ゼロ・決定論。gap_top の散らしに使う）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_splits_nonempty_lines() {
        let lines = run_command("printf 'a\\nbb\\nccc\\n'").unwrap();
        assert_eq!(lines, vec!["a", "bb", "ccc"]);
    }

    #[test]
    fn run_command_filters_blank_lines() {
        // 空行・空白のみの行は落とす。
        let lines = run_command("printf 'a\\n\\n  \\nb\\n'").unwrap();
        assert_eq!(lines, vec!["a", "b"]);
    }

    #[test]
    fn gap_top_for_is_deterministic_and_in_range() {
        let cfg = Config::default();
        let hi = cfg.rows - 1 - cfg.pipe_gap; // 17
        for line in [
            "total 56",
            "drwxr-xr-x  5 astel staff 160 crates",
            "-rw-r--r--  1 astel staff 2451 CLAUDE.md",
            "",
            "x",
        ] {
            let g = gap_top_for(line, cfg.rows, cfg.pipe_gap);
            assert!(
                (1..=hi).contains(&g),
                "gap_top {g} out of [1,{hi}] for {line:?}"
            );
            // 同じ行は同じ値（決定論）。
            assert_eq!(g, gap_top_for(line, cfg.rows, cfg.pipe_gap));
        }
    }

    #[test]
    fn build_course_maps_each_line_and_is_deterministic() {
        let cfg = Config::default();
        let lines: Vec<String> = ["alpha", "beta", "gamma"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let a = build_course(&lines, &cfg);
        assert_eq!(a.len(), lines.len());
        // 同じ入力 → 同じコース。
        assert_eq!(a, build_course(&lines, &cfg));
        // 各値が範囲内。
        let hi = cfg.rows - 1 - cfg.pipe_gap;
        assert!(a.iter().all(|&g| (1..=hi).contains(&g)));
    }
}
