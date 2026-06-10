//! core の状態を 1 フレームの文字グリッド（プレーンテキスト）へ変換する純粋関数。
//!
//! 色は付けない（描画時に付与する）。これにより判定（core の占有述語）と描画が
//! 同じセル定義を共有しつつ、ゴールデンテストが ANSI に汚されず安定する。

use flappy_core::{pipe_blocks_row, Game, Phase};

const BIRD_ALIVE: char = '●';
const BIRD_DEAD: char = '✕';
const PIPE: char = '█';
const GROUND: char = '─';

/// 文字列をグリッド行へ中央寄せで上書きする。
fn place_centered(row: &mut [char], text: &str, cols: u16) {
    let tw = text.chars().count();
    let start = (cols as usize).saturating_sub(tw) / 2;
    place_at(row, text, start);
}

/// 文字列をグリッド行の指定列へ上書きする。
fn place_at(row: &mut [char], text: &str, start: usize) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(cell) = row.get_mut(start + i) {
            *cell = ch;
        }
    }
}

/// 既知状態の core を 1 フレームの文字グリッドへ変換する。`rows` 行を `\n` で連結。
pub fn scene_to_string(game: &Game) -> String {
    let cfg = game.config();
    let (cols, rows) = (cfg.cols, cfg.rows);
    let mut grid = vec![vec![' '; cols as usize]; rows as usize];

    // 天井ライン（最上行）: web と同じく row 0 全幅に横帯を引き、その上に HUD を重ねる。
    for cell in grid[0].iter_mut() {
        *cell = GROUND;
    }

    // HUD（最上行）: 左に SCORE、右に BEST。
    let score_text = format!("SCORE {}", game.score);
    place_at(&mut grid[0], &score_text, 1);
    let best_text = format!("BEST {}", game.best);
    let best_start = (cols as usize).saturating_sub(best_text.chars().count() + 1);
    place_at(&mut grid[0], &best_text, best_start);

    // 地面ライン（最下行）。右端に version を控えめに重ねる（単一ソース = core）。
    if let Some(last) = grid.last_mut() {
        for cell in last.iter_mut() {
            *cell = GROUND;
        }
        let ver = format!("v{}", flappy_core::VERSION);
        let start = (cols as usize).saturating_sub(ver.chars().count() + 1);
        place_at(last, &ver, start);
    }

    // 棒（占有述語を判定と共有）。
    for p in game.pipes() {
        let c = p.x.round() as i32;
        if c >= 0 && (c as u16) < cols {
            for row in 0..rows as i32 {
                if pipe_blocks_row(p.gap_top, cfg.pipe_gap, rows, row) {
                    grid[row as usize][c as usize] = PIPE;
                }
            }
        }
    }

    // 鳥（衝突と同じ丸めのセル）。GameOver は ✕。
    let (bc, br) = game.bird_cell();
    if br < rows && bc < cols {
        let ch = if game.phase() == Phase::GameOver {
            BIRD_DEAD
        } else {
            BIRD_ALIVE
        };
        grid[br as usize][bc as usize] = ch;
    }

    // メッセージのオーバーレイ。
    match game.phase() {
        Phase::Ready => {
            place_centered(&mut grid[3], "F L A P P Y", cols);
            place_centered(&mut grid[8], "──  press SPACE  ──", cols);
        }
        Phase::GameOver => draw_gameover_box(&mut grid, cols, game.score),
        Phase::Playing => {}
    }

    grid.into_iter()
        .map(|row| row.into_iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// GameOver の罫線ボックスを中央へ描く（内側幅 20）。
fn draw_gameover_box(grid: &mut [Vec<char>], cols: u16, score: u32) {
    const INNER: usize = 20;
    let start = (cols as usize).saturating_sub(INNER + 2) / 2;
    let top = 2usize;

    let border_top: String = format!("╔{}╗", "═".repeat(INNER));
    let border_bottom: String = format!("╚{}╝", "═".repeat(INNER));
    let line = |inner: &str| -> String {
        // inner を中央寄せして 20 幅にし、両端に罫線を付ける。
        let tw = inner.chars().count();
        let pad = INNER.saturating_sub(tw);
        let left = pad / 2;
        let right = pad - left;
        format!("║{}{}{}║", " ".repeat(left), inner, " ".repeat(right))
    };

    let body = [
        border_top,
        line("GAME  OVER"),
        line(&format!("SCORE {score}")),
        line("SPACE / r : retry"),
        line("q         : quit"),
        border_bottom,
    ];
    for (i, text) in body.iter().enumerate() {
        if let Some(row) = grid.get_mut(top + i) {
            place_at(row, text, start);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flappy_core::{Config, Game};

    fn lines(s: &str) -> Vec<&str> {
        s.split('\n').collect()
    }

    /// 行末空白を除いて比較（コミット済みゴールデンが editor に削られても安定させる）。
    fn trim_trailing(s: &str) -> String {
        s.lines()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ready_frame_matches_golden() {
        let g = Game::new(Config::default(), 1);
        let scene = scene_to_string(&g);
        assert_eq!(
            trim_trailing(&scene),
            trim_trailing(include_str!("golden/ready.txt"))
        );
    }

    /// ゴールデン再生成用（レイアウト変更時に `cargo test -p flappy-term dump_golden -- --ignored`）。
    #[test]
    #[ignore]
    fn dump_golden() {
        let g = Game::new(Config::default(), 1);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/golden/ready.txt");
        std::fs::write(path, scene_to_string(&g)).unwrap();
    }

    #[test]
    fn ready_has_hud_bird_ground_and_message() {
        let g = Game::new(Config::default(), 1);
        let scene = scene_to_string(&g);
        let ls = lines(&scene);
        assert_eq!(ls.len(), 24);
        assert!(ls[0].contains("SCORE 0"));
        assert!(ls[0].contains("BEST 0"));
        // 天井ライン: HUD の左右の隙間は ─ で埋まる（web と見た目を揃える）。
        assert!(
            ls[0].contains('─'),
            "ceiling line should render as ─ on row 0"
        );
        // 鳥は (col 12, row 12)。
        assert_eq!(ls[12].chars().nth(12), Some('●'));
        // 地面ラインは ─ 基調で、右端に version を重ねる。
        assert!(ls[23].starts_with(&"─".repeat(40)));
        assert!(ls[23].contains(&format!("v{}", flappy_core::VERSION)));
        assert!(scene.contains("F L A P P Y"));
        assert!(scene.contains("press SPACE"));
    }

    #[test]
    fn version_is_drawn_from_core_const() {
        let g = Game::new(Config::default(), 1);
        let scene = scene_to_string(&g);
        // ハードコードせず core の VERSION を参照していること。
        assert!(scene.contains(&format!("v{}", flappy_core::VERSION)));
    }

    #[test]
    fn playing_drops_message_and_keeps_bird() {
        let mut g = Game::new(Config::default(), 1);
        g.flap(); // Ready → Playing
        let scene = scene_to_string(&g);
        assert_eq!(g.phase(), Phase::Playing);
        assert!(
            !scene.contains("press SPACE"),
            "no menu message while playing"
        );
        assert!(!scene.contains("F L A P P Y"));
        assert_eq!(lines(&scene)[12].chars().nth(12), Some('●'));
    }

    #[test]
    fn pipe_renders_as_block_when_on_screen() {
        // 棒が画面内（col < cols）に来るまで hover で生かして tick する。
        let center = Config::default().rows / 2;
        let mut g = Game::new(Config::default(), 1);
        let mut found = false;
        for _ in 0..400 {
            if g.bird_cell().1 >= center {
                g.flap();
            }
            g.tick();
            if scene_to_string(&g).contains('█') {
                found = true;
                break;
            }
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert!(found, "a pipe should render as █ once on screen");
    }

    #[test]
    fn gameover_shows_box_and_dead_bird() {
        // 落下して地面に激突 → GameOver。
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        for _ in 0..300 {
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        let scene = scene_to_string(&g);
        assert!(scene.contains("GAME  OVER"));
        assert!(scene.contains("SPACE / r : retry"));
        assert!(scene.contains("q         : quit"));
        assert!(scene.contains('✕'), "dead bird should render as ✕");
        // スコアボックスに現在スコア（落下死なので 0）。
        assert!(scene.contains("SCORE 0"));
    }
}
