//! flappy-term（bin 名 `flappy`）— crossterm でターミナル描画する薄いレンダラ。
//!
//! 本 issue (#9) では端末ライフサイクル（alternate screen / raw mode / カーソル非表示 /
//! mouse capture）を RAII ガードと panic hook で安全に管理し、q / Esc で抜ける空ループ
//! までを用意する。ゲーム描画・入力・ループは後続 issue で追加する。

use std::io::{self, Write};
use std::panic;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};

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

fn main() -> io::Result<()> {
    // panic 時はまず端末を復帰してから既定 hook（バックトレース表示）を呼ぶ。
    // `panic = "abort"` は設定しないこと（unwinding / Drop が走らなくなる）。
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    let _guard = TerminalGuard::enter()?;

    // 空ループ: q / Esc で抜ける（ゲーム本体は後続 issue で実装）。
    loop {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
    // ここで `_guard` が drop され端末が復帰する。
}
