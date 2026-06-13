//! flappy-term（bin 名 `flappy`）— crossterm でターミナル描画する薄いレンダラ。
//!
//! 端末ライフサイクル（alternate screen / raw mode / カーソル非表示 / mouse capture）を
//! RAII ガードと panic hook で安全に管理する。core 状態をフレームへ変換する純粋関数
//! [`scene::render`] を一括描画し、[`input`] で Space/クリック/r/q/Esc を core
//! 操作へルーティングし、実時間を蓄積して固定 [`flappy_core::DT`] 刻みで物理を進める
//! ループを回す。端末サイズに応じてセンタリング（レターボックス）し、最小サイズ
//! (64×24) 未満ではプレイを止めてリサイズを促す（[`layout`]）。

mod headless;
mod input;
mod layout;
mod scene;

use std::io::{self, Write};
use std::panic;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{cursor, execute, queue};

use flappy_core::{Config, Game, DT};

use layout::Layout;

/// 端末を元の状態へ戻す（best-effort、エラーは無視）。Drop と panic hook で共有する。
fn restore_terminal() {
    let mut out = io::stdout();
    let _ = execute!(out, LeaveAlternateScreen, DisableMouseCapture, cursor::Show);
    let _ = disable_raw_mode();
    let _ = out.flush();
}

/// 端末ライフサイクルの RAII ガード。`enter()` で入場し、`Drop` で確実に復帰する。
/// crossterm 0.29 に raw mode の RAII は無いため自前で持つ。
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// `Paint` タグを端末色へ写す。生存鳥・None は端末既定色（web の鳥 #333 と揃える）、
/// 死亡鳥は赤（web の #c0392b と揃える）、棒は緑。
fn paint_color(paint: scene::Paint) -> Option<Color> {
    match paint {
        scene::Paint::Pipe => Some(Color::Green),
        scene::Paint::BirdDead => Some(Color::Red),
        scene::Paint::Bird | scene::Paint::None => None,
    }
}

/// 1 フレームを `(ox, oy)` を左上として一括描画する。棒は緑、鳥・他は端末既定色。
/// 行内で同色のランをまとめ、色が変わるときだけエスケープを出して描画量を抑える。
fn draw_scene(out: &mut impl Write, game: &Game, ox: u16, oy: u16) -> io::Result<()> {
    let frame = scene::render(game);
    for (y, (line, paints)) in frame.chars.iter().zip(frame.paint.iter()).enumerate() {
        queue!(out, cursor::MoveTo(ox, oy + y as u16))?;
        // 現在出力中の色（None = 端末既定色）。行頭は既定色から始める。
        let mut cur: Option<Color> = None;
        for (&ch, &p) in line.iter().zip(paints.iter()) {
            let want = paint_color(p);
            if want != cur {
                match want {
                    Some(c) => queue!(out, SetForegroundColor(c))?,
                    None => queue!(out, ResetColor)?,
                }
                cur = want;
            }
            queue!(out, Print(ch))?;
        }
        // 行末で着色を残さない。
        if cur.is_some() {
            queue!(out, ResetColor)?;
        }
    }
    out.flush()
}

/// 文字列の端末表示幅（全角=2, 半角=1）を返す簡易計算。`unicode-width` 依存を避け、
/// CJK 系の主要レンジだけを 2 幅として扱う最小実装（draw_pause の和文メッセージ用）。
fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// 1 文字の表示幅。East Asian Wide/Fullwidth に概ね相当する主要レンジを 2 幅とする。
fn char_width(c: char) -> usize {
    let cp = c as u32;
    let wide = matches!(cp,
        0x1100..=0x115F      // Hangul Jamo
        | 0x2E80..=0xA4CF    // CJK 部首〜CJK統合漢字〜Yi（記号・かな・漢字を広くカバー）
        | 0xAC00..=0xD7A3    // Hangul 音節
        | 0xF900..=0xFAFF    // CJK 互換漢字
        | 0xFE30..=0xFE4F    // CJK 互換形
        | 0xFF00..=0xFF60    // 全角形
        | 0xFFE0..=0xFFE6    // 全角記号
    );
    if wide {
        2
    } else {
        1
    }
}

/// 端末が最小サイズ未満のときのポーズ表示（中央にメッセージ）。
fn draw_pause(out: &mut impl Write, term: (u16, u16)) -> io::Result<()> {
    let (tw, th) = term;
    let msg = "端末を 64x24 以上にしてください";
    // 端末は全角を 2 セル幅で表示するため、コードポイント数ではなく表示幅で中央寄せする。
    let x = (tw as usize).saturating_sub(display_width(msg)) / 2;
    queue!(out, cursor::MoveTo(x as u16, th / 2), Print(msg))?;
    out.flush()
}

fn seed_from_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// `args` から `flag` の次トークンを取り出す（`--seed 1` の `1`）。
fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

const USAGE: &str = "flappy - ターミナルで遊ぶ Flappy Bird 風ドットゲーム

USAGE:
    flappy                                   TUI でプレイ
    flappy --headless [--seed S] [--frames N]
                                             決定論 autopilot で N フレーム実行し
                                             最終スコアを stdout に出力（既定 S=1, N=600）
    flappy -h, --help                        このヘルプを表示
    flappy -V, --version                     バージョンを表示";

/// `flag` の値を parse する。値が不正なら usage 誘導つきで非ゼロ終了（silent fallback しない）。
fn parse_flag_or_exit<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    match arg_value(args, flag) {
        None => default,
        Some(s) => s.parse().unwrap_or_else(|_| {
            eprintln!("error: {flag} の値が不正です: {s:?}（--help 参照）");
            std::process::exit(2);
        }),
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // --help / --version は TUI（alternate screen）に入らず即終了する（CLI の慣習）。
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("flappy {}", flappy_core::VERSION);
        return Ok(());
    }

    // headless モード: TTY 不要・端末ガード非経由で N フレーム自動実行しスコアを stdout 出力。
    if args.iter().any(|a| a == "--headless") {
        let seed = parse_flag_or_exit(&args, "--seed", 1);
        let frames = parse_flag_or_exit(&args, "--frames", 600);
        println!("{}", headless::run_headless(seed, frames));
        return Ok(());
    }

    // panic 時はまず端末を復帰してから既定 hook（バックトレース表示）を呼ぶ。
    // `panic = "abort"` は設定しないこと（unwinding / Drop が走らなくなる）。
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    let _guard = TerminalGuard::enter()?;

    let mut game = Game::new(Config::default(), seed_from_clock());
    let grid = (Config::default().cols, Config::default().rows);

    let mut out = io::stdout();
    execute!(out, Clear(ClearType::All))?;

    // 実時間を蓄積し固定 DT 刻みで物理を進める（描画頻度に依存しない＝決定論）。
    let mut last_size = size()?;
    let mut last = Instant::now();
    let mut acc = 0.0f32;

    'game: loop {
        // 入力を非ブロッキングで全て取り出して適用。
        while event::poll(Duration::ZERO)? {
            let ev = event::read()?;
            if let Some(input) = input::classify(&ev) {
                match input::route(input, game.phase()) {
                    input::Action::Flap => game.flap(),
                    input::Action::Restart => game.restart(),
                    input::Action::Quit => break 'game,
                }
            }
        }

        // サイズ変化時は全消去して再センタリング（ゲーム状態は不変）。
        let term = size()?;
        if term != last_size {
            execute!(out, Clear(ClearType::All))?;
            last_size = term;
        }

        match layout::compute_layout(term, grid) {
            Layout::TooSmall => {
                // ポーズ: 物理を進めず、蓄積時間もリセット（復帰時に飛ばさない）。
                draw_pause(&mut out, term)?;
                last = Instant::now();
                acc = 0.0;
            }
            Layout::Fit { ox, oy } => {
                let now = Instant::now();
                // 1 フレーム上限 0.10s（=6 tick）で spiral of death を防ぐ。
                acc += (now - last).as_secs_f32().min(0.10);
                last = now;
                while acc >= DT {
                    game.tick();
                    acc -= DT;
                }
                draw_scene(&mut out, &game, ox, oy)?;
            }
        }

        // 描画ペース ~60Hz（物理は固定 DT なのでこの値に依存しない）。
        std::thread::sleep(Duration::from_millis(16));
    }

    Ok(())
    // ここで `_guard` が drop され端末が復帰する。
}

#[cfg(test)]
mod tests {
    use super::{char_width, display_width};

    #[test]
    fn ascii_is_one_cell_each() {
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("64x24"), 5);
        assert_eq!(display_width("F L A P P Y"), 11);
        assert_eq!(char_width(' '), 1);
        assert_eq!(char_width('A'), 1);
    }

    #[test]
    fn cjk_is_two_cells_each() {
        // ひらがな・漢字はそれぞれ 2 幅。
        assert_eq!(char_width('端'), 2);
        assert_eq!(char_width('を'), 2);
        assert_eq!(display_width("端末"), 4);
        assert_eq!(display_width("以上"), 4);
    }

    #[test]
    fn pause_message_width_counts_fullwidth() {
        // draw_pause の実メッセージ。和文 12 文字(=24) + 半角 " 64x24 "(=7) = 31。
        let msg = "端末を 64x24 以上にしてください";
        assert_eq!(msg.chars().count(), 19);
        assert_eq!(display_width(msg), 31);
        // コードポイント数より表示幅のほうが大きい（バグの本質）。
        assert!(display_width(msg) > msg.chars().count());
    }
}
