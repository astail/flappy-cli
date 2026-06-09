//! flappy-term（bin 名 `flappy`）— crossterm でターミナル描画する薄いレンダラ。
//!
//! 端末ライフサイクル（alternate screen / raw mode / カーソル非表示 / mouse capture）を
//! RAII ガードと panic hook で安全に管理する。core 状態を文字グリッドへ変換する純粋関数
//! [`scene::scene_to_string`] を左上から一括描画し、[`input`] で Space/クリック/r/q/Esc を
//! core 操作へルーティングする。固定 DT の物理 tick ループは後続 issue (#12) で追加する。

mod input;
mod scene;

use std::io::{self, Write};
use std::panic;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute, queue};

use flappy_core::{Config, Game};

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

/// 1 フレームを左上から一括描画する。棒（`█`）のみ緑、それ以外は端末既定色。
fn draw_scene(out: &mut impl Write, game: &Game) -> io::Result<()> {
    let scene = scene::scene_to_string(game);
    queue!(out, cursor::MoveTo(0, 0))?;
    for (y, line) in scene.lines().enumerate() {
        queue!(out, cursor::MoveTo(0, y as u16))?;
        for ch in line.chars() {
            if ch == '█' {
                queue!(out, SetForegroundColor(Color::Green), Print(ch), ResetColor)?;
            } else {
                queue!(out, Print(ch))?;
            }
        }
        queue!(out, Clear(ClearType::UntilNewLine))?;
    }
    out.flush()
}

fn seed_from_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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

    // ゲーム状態。本 issue では入力ルーティングで状態を更新し再描画する。
    // 固定 DT の物理 tick ループは後続 issue（#12）で追加する。
    let mut game = Game::new(Config::default(), seed_from_clock());

    let mut out = io::stdout();
    execute!(out, Clear(ClearType::All))?;
    draw_scene(&mut out, &game)?;

    // 入力ルーティング: Space/クリック → GameOver なら restart 他は flap、r → restart、q/Esc → 終了。
    loop {
        if event::poll(Duration::from_millis(100))? {
            let ev = event::read()?;
            if let Some(input) = input::classify(&ev) {
                match input::route(input, game.phase()) {
                    input::Action::Flap => game.flap(),
                    input::Action::Restart => game.restart(),
                    input::Action::Quit => break,
                }
                draw_scene(&mut out, &game)?;
            } else if let Event::Resize(_, _) = ev {
                draw_scene(&mut out, &game)?;
            }
        }
    }

    Ok(())
    // ここで `_guard` が drop され端末が復帰する。
}
