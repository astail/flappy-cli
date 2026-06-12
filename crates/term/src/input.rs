//! 端末入力（キー / マウス）を core 操作へ対応づける純粋なルーティング。
//!
//! crossterm の生イベントを抽象 [`Input`] に分類（[`classify`]）し、現在の [`Phase`] に
//! 応じて具体的な [`Action`] へ振り分ける（[`route`]）。どちらも I/O を持たないので
//! 単体テスト可能。DESIGN §1 の入力→効果表に対応する。

use crossterm::event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use flappy_core::Phase;

/// 抽象化した入力種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// SPACE / クリック・タップ（主操作）。
    Primary,
    /// r（リスタートのショートカット）。
    Restart,
    /// q / Esc（終了）。
    Quit,
}

/// 入力が core / ループに与える効果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Flap,
    Restart,
    Quit,
}

/// crossterm イベントを抽象入力へ分類する。対象外は `None`。
pub fn classify(event: &Event) -> Option<Input> {
    match event {
        Event::Key(k) => {
            if k.kind != KeyEventKind::Press {
                return None; // Repeat/Release は無視（押下のみ操作に対応）
            }
            match k.code {
                KeyCode::Char(' ') => Some(Input::Primary),
                KeyCode::Char('r') | KeyCode::Char('R') => Some(Input::Restart),
                KeyCode::Char('q') | KeyCode::Char('Q') => Some(Input::Quit),
                KeyCode::Esc => Some(Input::Quit),
                _ => None,
            }
        }
        Event::Mouse(m) => match m.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(Input::Primary),
            _ => None,
        },
        _ => None,
    }
}

/// 現在の状態に応じて入力を core 操作へ振り分ける（DESIGN §1）。
/// SPACE/クリックは GameOver ならリスタート、それ以外はフラップ（Ready は初回フラップで開始）。
pub fn route(input: Input, phase: Phase) -> Action {
    match input {
        Input::Primary => {
            if phase == Phase::GameOver {
                Action::Restart
            } else {
                Action::Flap
            }
        }
        Input::Restart => Action::Restart,
        Input::Quit => Action::Quit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers, MouseEvent};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn key_with_kind(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
    }

    fn left_click() -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn classify_keys_and_mouse() {
        assert_eq!(classify(&key(KeyCode::Char(' '))), Some(Input::Primary));
        assert_eq!(classify(&left_click()), Some(Input::Primary));
        assert_eq!(classify(&key(KeyCode::Char('r'))), Some(Input::Restart));
        assert_eq!(classify(&key(KeyCode::Char('R'))), Some(Input::Restart));
        assert_eq!(classify(&key(KeyCode::Char('q'))), Some(Input::Quit));
        assert_eq!(classify(&key(KeyCode::Esc)), Some(Input::Quit));
        // 対象外。
        assert_eq!(classify(&key(KeyCode::Char('x'))), None);
        assert_eq!(classify(&key(KeyCode::Enter)), None);
    }

    #[test]
    fn classify_ignores_non_press_key_events() {
        // Windows コンソールや kitty keyboard protocol 環境では
        // Release/Repeat イベントも飛んでくるため、Press のみ処理する。
        for code in [KeyCode::Char(' '), KeyCode::Char('r'), KeyCode::Char('q')] {
            assert_eq!(classify(&key_with_kind(code, KeyEventKind::Release)), None);
            assert_eq!(classify(&key_with_kind(code, KeyEventKind::Repeat)), None);
            assert!(classify(&key_with_kind(code, KeyEventKind::Press)).is_some());
        }
    }

    #[test]
    fn primary_flaps_unless_gameover() {
        assert_eq!(route(Input::Primary, Phase::Ready), Action::Flap);
        assert_eq!(route(Input::Primary, Phase::Playing), Action::Flap);
        assert_eq!(route(Input::Primary, Phase::GameOver), Action::Restart);
    }

    #[test]
    fn restart_and_quit_are_phase_independent() {
        for phase in [Phase::Ready, Phase::Playing, Phase::GameOver] {
            assert_eq!(route(Input::Restart, phase), Action::Restart);
            assert_eq!(route(Input::Quit, phase), Action::Quit);
        }
    }
}
