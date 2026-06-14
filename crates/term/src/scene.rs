//! core の状態を 1 フレームへ変換する純粋関数。
//!
//! 動く要素（鳥・棒）は Braille（1 セル = 2×4 ドット）でサブセル描画し、横 2 倍・縦 4 倍の
//! 実効解像度で滑らかに動かす。テキスト（HUD/天井・地面ライン/メッセージ/GameOver ボックス）は
//! 従来どおり文字で上書きする。色は付けない（描画時に [`Paint`] タグから付与する）。これにより
//! 判定（core の占有述語）と描画が同じセル定義を共有しつつ、ゴールデンテストが ANSI に汚されず
//! 安定する。

use flappy_core::{
    pipe_blocks_row, Game, Phase, GAMEOVER_RETRY_HINT, GAMEOVER_TITLE, READY_HINT, READY_TITLE,
};

const BIRD: char = '●';
const BIRD_DEAD: char = '✕';
const GROUND: char = '─';

/// 1 セルの塗り分けタグ（色は描画時に付与）。
#[derive(Clone, Copy, PartialEq)]
pub enum Paint {
    None,
    Pipe,
    Bird,
    /// 死亡した鳥（✕）。生存鳥（既定色）と区別して赤で描く（web の #c0392b と揃える）。
    BirdDead,
}

/// 1 フレーム。`chars` が描画グリフ、`paint` が同サイズの塗り分けタグ。
pub struct Frame {
    pub chars: Vec<Vec<char>>,
    pub paint: Vec<Vec<Paint>>,
}

impl Frame {
    /// chars を `\n` 連結したプレーンテキスト（テスト/ゴールデン用）。
    #[cfg(test)]
    pub fn to_text(&self) -> String {
        self.chars
            .iter()
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 2×4 ドットブロックを Braille コードポイントへパックする。
/// ビット対応 (dx,dy): (0,0)=0x01 (0,1)=0x02 (0,2)=0x04 (1,0)=0x08
/// (1,1)=0x10 (1,2)=0x20 (0,3)=0x40 (1,3)=0x80。mask==0 は空白。
fn braille(mask: u8) -> char {
    if mask == 0 {
        ' '
    } else {
        char::from_u32(0x2800 + mask as u32).unwrap()
    }
}

/// ドット (dx in 0..2, dy in 0..4) に対応するビット。
fn dot_bit(dx: usize, dy: usize) -> u8 {
    match (dx, dy) {
        (0, 0) => 0x01,
        (0, 1) => 0x02,
        (0, 2) => 0x04,
        (1, 0) => 0x08,
        (1, 1) => 0x10,
        (1, 2) => 0x20,
        (0, 3) => 0x40,
        (1, 3) => 0x80,
        _ => 0,
    }
}

/// 文字列をグリッド行へ中央寄せで上書きする（paint は None に戻す）。
fn place_centered(chars: &mut [char], paint: &mut [Paint], text: &str, cols: u16) {
    let tw = text.chars().count();
    let start = (cols as usize).saturating_sub(tw) / 2;
    place_at(chars, paint, text, start);
}

/// 文字列をグリッド行の指定列へ上書きする（paint は None に戻す）。
fn place_at(chars: &mut [char], paint: &mut [Paint], text: &str, start: usize) {
    for (i, ch) in text.chars().enumerate() {
        if let Some(cell) = chars.get_mut(start + i) {
            *cell = ch;
            paint[start + i] = Paint::None;
        }
    }
}

/// 既知状態の core を 1 フレームへ変換する。
///
/// `course_lines` が `None`（通常）なら棒を Braille サブセルで描く。`Some(lines)`（term の
/// `--cmd` モード）なら棒を「その行の文字を縦に敷き詰めた文字壁」で描く（`lines[course_idx]`）。
/// 鳥・HUD・天井/地面ライン・メッセージは [`overlay_text`] で両モード共通に上書きする。
pub fn render(game: &Game, course_lines: Option<&[String]>) -> Frame {
    let cfg = game.config();
    let (cols, rows) = (cfg.cols as usize, cfg.rows as usize);

    let mut chars = vec![vec![' '; cols]; rows];
    let mut paint = vec![vec![Paint::None; cols]; rows];

    match course_lines {
        None => render_pipes_braille(game, &mut chars, &mut paint, cols, rows),
        Some(lines) => render_pipes_text(game, lines, &mut chars, &mut paint, cols, rows),
    }

    // 鳥は Braille ブロブではなく ● の 1 文字で描く（overlay_text 参照。web の塗り円と
    // 見た目を揃える。GameOver の ✕ も同様に文字で描く）。テキストレイヤーで上書き。
    overlay_text(game, &mut chars, &mut paint, cfg.cols, cfg.rows);

    Frame { chars, paint }
}

/// 棒を Braille サブセル（1 セル = 横2×縦4 ドット）で描く（通常モード。web と見た目を揃える）。
///
/// dot-x = round(x*2) から幅 2 ドット（=1 セル幅）。塞ぐセル行の dot-y 4 本を立てる。
/// 横は dot 解像度（既知の制約: 衝突は round(p.x) の 1 セルで判定されるため視覚との差は
/// 最大 1/4 セル。描画セルは衝突セル round(p.x) を常に含むので「触れて見えないのに死ぬ」
/// ことはなく、ずれは「触れて見えても生きている」側にだけ倒れる）。
fn render_pipes_braille(
    game: &Game,
    chars: &mut [Vec<char>],
    paint: &mut [Vec<Paint>],
    cols: usize,
    rows: usize,
) {
    let cfg = game.config();
    let dot_w = cols * 2;
    let mut dots = vec![vec![false; dot_w]; rows * 4];

    for p in game.pipes() {
        let dx0 = (p.x * 2.0).round() as i32;
        for dx in [dx0, dx0 + 1] {
            if dx < 0 || dx as usize >= dot_w {
                continue;
            }
            for row in 0..rows as i32 {
                if pipe_blocks_row(p.gap_top, cfg.pipe_gap, cfg.rows, row) {
                    for k in 0..4 {
                        dots[row as usize * 4 + k][dx as usize] = true;
                    }
                }
            }
        }
    }

    // ドットを 2×4 ブロックごとに Braille へパック。立っているセルに塗り分けを付ける。
    for r in 0..rows {
        for c in 0..cols {
            let mut mask = 0u8;
            for dy in 0..4 {
                for dx in 0..2 {
                    if dots[r * 4 + dy][c * 2 + dx] {
                        mask |= dot_bit(dx, dy);
                    }
                }
            }
            if mask != 0 {
                // dots に立つのは棒だけ（鳥は overlay_text で ● 描画）なので常に Pipe。
                chars[r][c] = braille(mask);
                paint[r][c] = Paint::Pipe;
            }
        }
    }
}

/// 棒を「その行の文字を縦に敷き詰めた文字壁」で描く（term の `--cmd` モード専用）。
///
/// 棒が塞ぐ各行に、その棒由来の行（`Pipe.course_idx` → `lines[idx]`）の文字を 1 つ置く。
/// 行が棒より短ければ繰り返す。行の文字が画面全体を縦に走り、隙間（穴）だけ抜けたように見える。
/// 横位置は衝突と同じ `round(p.x)` の 1 セル（Braille の 1/2 セル平滑は course 時は無し。
/// 衝突も `round(p.x)` なので見た目と判定はセル単位で一致する）。
fn render_pipes_text(
    game: &Game,
    lines: &[String],
    chars: &mut [Vec<char>],
    paint: &mut [Vec<Paint>],
    cols: usize,
    rows: usize,
) {
    // `render` の pub 引数として `Some(&[])` を受け取りうるため、後段 `% lines.len()` の
    // 0 除算を防ぐ境界ガード（実プレイでは main の course_lines_or_exit が 0 行を exit 2 で弾く）。
    if lines.is_empty() {
        return;
    }
    let cfg = game.config();
    for p in game.pipes() {
        let col = p.x.round() as i32;
        if col < 0 || col as usize >= cols {
            continue;
        }
        let glyphs: Vec<char> = lines[p.course_idx % lines.len()].chars().collect();
        if glyphs.is_empty() {
            continue;
        }
        for row in 1..rows as i32 {
            if pipe_blocks_row(p.gap_top, cfg.pipe_gap, cfg.rows, row) {
                // row は塞ぐ範囲なので必ず >= 1。row-1 を行内オフセットとし、穴を挟んでも
                // 文字列が縦に通って見えるよう row から直接 index を引く（cycle で埋める）。
                chars[row as usize][col as usize] = glyphs[(row as usize - 1) % glyphs.len()];
                paint[row as usize][col as usize] = Paint::Pipe;
            }
        }
    }
}

/// テキストレイヤー（天井・地面ライン / HUD / メッセージ / GameOver ボックス / 死亡鳥）を上書きする。
fn overlay_text(
    game: &Game,
    chars: &mut [Vec<char>],
    paint: &mut [Vec<Paint>],
    cols: u16,
    rows: u16,
) {
    let cols_u = cols as usize;

    // 天井ライン（最上行）: row 0 全幅に横帯を引き、その上に HUD を重ねる。
    for (cell, p) in chars[0].iter_mut().zip(paint[0].iter_mut()) {
        *cell = GROUND;
        *p = Paint::None;
    }
    let score_text = format!("SCORE {}", game.score());
    place_at(&mut chars[0], &mut paint[0], &score_text, 1);
    let best_text = format!("BEST {}", game.best());
    let best_start = cols_u.saturating_sub(best_text.chars().count() + 1);
    place_at(&mut chars[0], &mut paint[0], &best_text, best_start);

    // 地面ライン（最下行）。右端に version を控えめに重ねる（単一ソース = core）。
    let last = rows as usize - 1;
    for (cell, p) in chars[last].iter_mut().zip(paint[last].iter_mut()) {
        *cell = GROUND;
        *p = Paint::None;
    }
    let ver = format!("v{}", flappy_core::VERSION);
    let ver_start = cols_u.saturating_sub(ver.chars().count() + 1);
    let (last_chars, last_paint) = (&mut chars[last], &mut paint[last]);
    place_at(last_chars, last_paint, &ver, ver_start);

    // メッセージのオーバーレイ。
    match game.phase() {
        Phase::Ready => {
            place_centered(&mut chars[3], &mut paint[3], READY_TITLE, cols);
            place_centered(&mut chars[8], &mut paint[8], READY_HINT, cols);
        }
        Phase::GameOver => {
            draw_gameover_box(chars, paint, cols, game.score());
            // 死亡した鳥は ✕ の文字で表す（render はブロブを描かない）。棒セルの上で
            // 死んだ場合も ✕ が棒色にならないよう paint を BirdDead にする（赤で描く）。
            let (bc, br) = game.bird_cell();
            // 天井死は bird_cell の row が 0 にクランプされる（core lib.rs の max(0)）。
            // ✕ が天井ライン/HUD 帯（row 0）を潰さないよう、プレイエリア最上行（row 1）へ寄せる。
            let br = br.max(1);
            if (br as usize) < rows as usize && bc < cols {
                chars[br as usize][bc as usize] = BIRD_DEAD;
                paint[br as usize][bc as usize] = Paint::BirdDead;
            }
        }
        Phase::Playing => {}
    }

    // 生存中の鳥は ● の 1 文字で描く（render はブロブを描かない。web の塗り円と見た目を揃える）。
    // 行は bird_cell()（衝突と同じ round 行）。死亡 ✕ と同様、天井死クランプ等で row 0 に来ても
    // 天井ライン/HUD 帯を潰さないよう row 1 以上へ寄せる。
    if game.phase() != Phase::GameOver {
        let (bc, br) = game.bird_cell();
        let br = br.max(1);
        if (br as usize) < rows as usize && bc < cols {
            chars[br as usize][bc as usize] = BIRD;
            paint[br as usize][bc as usize] = Paint::Bird;
        }
    }
}

/// 既知状態の core を 1 フレームの文字グリッドへ変換する（プレーンテキスト、テスト/ゴールデン用）。
#[cfg(test)]
pub fn scene_to_string(game: &Game) -> String {
    render(game, None).to_text()
}

/// GameOver の罫線ボックスを中央へ描く（内側幅 = 最長行の retry 案内に合わせる）。
/// 文言は core の定数を参照する（#76: web と同一ソースで文言ズレを防ぐ）。
fn draw_gameover_box(chars: &mut [Vec<char>], paint: &mut [Vec<Paint>], cols: u16, score: u32) {
    let inner_w = GAMEOVER_RETRY_HINT.chars().count();
    let start = (cols as usize).saturating_sub(inner_w + 2) / 2;
    let top = 2usize;

    let border_top: String = format!("╔{}╗", "═".repeat(inner_w));
    let border_bottom: String = format!("╚{}╝", "═".repeat(inner_w));
    let line = |inner: &str| -> String {
        // inner を中央寄せして inner_w 幅にし、両端に罫線を付ける。
        let tw = inner.chars().count();
        let pad = inner_w.saturating_sub(tw);
        let left = pad / 2;
        let right = pad - left;
        format!("║{}{}{}║", " ".repeat(left), inner, " ".repeat(right))
    };

    let body = [
        border_top,
        line(GAMEOVER_TITLE),
        line(&format!("SCORE {score}")),
        line(GAMEOVER_RETRY_HINT),
        // quit は term 固有（web に終了概念がない。DESIGN §2 の許容差）。
        // ':' の位置を retry 行に揃える。
        line("q                 : quit"),
        border_bottom,
    ];
    for (i, text) in body.iter().enumerate() {
        let row = top + i;
        if row < chars.len() {
            place_at(&mut chars[row], &mut paint[row], text, start);
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

    /// ゴールデン比較用に version 表示を固定トークンへ置換（version bump のたびの
    /// ゴールデン手動更新を不要にする）。実 version が描画されること自体は
    /// version_is_drawn_from_core_const が別途守る。
    fn mask_version(s: &str) -> String {
        s.replace(&format!("v{}", flappy_core::VERSION), "vX.Y.Z")
    }

    #[test]
    fn ready_frame_matches_golden() {
        let g = Game::new(Config::default(), 1);
        let scene = scene_to_string(&g);
        assert_eq!(
            mask_version(&trim_trailing(&scene)),
            trim_trailing(include_str!("golden/ready.txt"))
        );
    }

    /// ゴールデン再生成用（レイアウト変更時に `cargo test -p flappy-term dump_golden -- --ignored`）。
    /// version はトークン化して書き出す（ready_frame_matches_golden の mask_version と対）。
    #[test]
    #[ignore]
    fn dump_golden() {
        let g = Game::new(Config::default(), 1);
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/golden/ready.txt");
        std::fs::write(path, mask_version(&scene_to_string(&g))).unwrap();
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
        // 鳥は col 12・bird_y=12.0 → bird_cell row 12。● の 1 文字で描かれる。
        let bird = ls[12].chars().nth(12).unwrap();
        assert_eq!(bird, BIRD, "bird cell should render as ●, got {bird:?}");
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
        // 1 tick も進めていないので鳥は bird_y=12.0 のまま row 12 col 12。
        let bird = lines(&scene)[12].chars().nth(12).unwrap();
        assert_eq!(bird, BIRD, "bird should remain a ● glyph while playing");
    }

    #[test]
    fn pipe_renders_as_braille_when_on_screen() {
        // 棒が画面内（col < cols）に来るまで hover で生かして tick する。
        let center = Config::default().rows / 2;
        let mut g = Game::new(Config::default(), 1);
        let mut found = false;
        for _ in 0..400 {
            if g.bird_cell().1 >= center {
                g.flap();
            }
            g.tick();
            // 棒セル（paint==Pipe）が存在すれば成功。
            let frame = render(&g, None);
            if frame.paint.iter().flatten().any(|p| *p == Paint::Pipe) {
                found = true;
                break;
            }
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert!(
            found,
            "a pipe should render with Paint::Pipe once on screen"
        );
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
        assert!(scene.contains(GAMEOVER_TITLE));
        assert!(scene.contains(GAMEOVER_RETRY_HINT));
        assert!(scene.contains("q                 : quit"));
        assert!(scene.contains('✕'), "dead bird should render as ✕");
        // スコアボックスに現在スコア（落下死なので 0）。
        assert!(scene.contains("SCORE 0"));
    }

    #[test]
    fn dead_bird_is_tagged_bird_dead() {
        // 死亡鳥セルが Paint::BirdDead でタグ付けされること（term 赤 / web #c0392b の色退行ガード）。
        // scene_to_string は chars のみで paint を捨てるため、paint タグは render() で直接検証する。
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        for _ in 0..300 {
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        let frame = render(&g, None);
        assert!(
            frame.paint.iter().flatten().any(|p| *p == Paint::BirdDead),
            "dead bird cell should be tagged Paint::BirdDead"
        );
    }

    #[test]
    fn ceiling_death_marker_clamps_to_play_area_top() {
        // 連打で上昇 → 天井死。死亡マーカー（✕）は天井ライン/HUD 帯（row 0）ではなく
        // プレイエリア最上行（row 1）に出ること（issue #112: term/web で一致）。
        let mut g = Game::new(Config::default(), 1);
        for _ in 0..300 {
            g.flap();
            g.tick();
            if g.phase() == Phase::GameOver {
                break;
            }
        }
        assert_eq!(g.phase(), Phase::GameOver);
        // 天井死の証跡: bird_cell の row は max(0) で 0 にクランプされる。
        assert_eq!(
            g.bird_cell().1,
            0,
            "ceiling death clamps bird_cell row to 0"
        );
        let frame = render(&g, None);
        assert!(
            !frame.chars[0].contains(&BIRD_DEAD),
            "death marker must not land on row 0 (ceiling line / HUD)"
        );
        assert!(
            frame.chars[1].contains(&BIRD_DEAD),
            "death marker must be clamped to play area top (row 1)"
        );
    }

    #[test]
    fn braille_packs_mask_to_codepoint() {
        assert_eq!(braille(0), ' ');
        assert_eq!(braille(0x01), '\u{2801}');
        assert_eq!(braille(0xFF), '\u{28FF}');
        // 縦 2 ドット帯（左列 dy=0,1 = 0x01|0x02 = 0x03）。
        assert_eq!(braille(0x03), '\u{2803}');
    }

    #[test]
    fn bird_renders_as_single_circle_while_drifting() {
        // 生存中の鳥は常に ● の 1 文字（= 1 つの円）。上昇〜落下で行が変わっても、
        // 鳥セルは bird_cell() の round 行 col 12 に 1 つだけ ● が出る（複数ドットにならない）。
        let mut g = Game::new(Config::default(), 1);
        g.flap();
        for _ in 0..30 {
            g.tick();
            if g.phase() != Phase::Playing {
                break;
            }
            let frame = render(&g, None);
            let row = g.bird_cell().1.max(1) as usize;
            assert_eq!(
                frame.chars[row][12], BIRD,
                "bird should render as a single ●"
            );
            // 鳥は ● ちょうど 1 つ（旧ブロブのような 4 ドットにならない）。
            let bird_count = frame.chars.iter().flatten().filter(|&&c| c == BIRD).count();
            assert_eq!(bird_count, 1, "exactly one ● should be drawn");
        }
    }

    #[test]
    fn course_mode_draws_text_wall_with_gap() {
        // --cmd モード: 棒セルに行の文字（cycle）が出て、穴 [gap_top, gap_top+gap) は空白。
        // Braille 棒（U+2800..U+28FF）は出ない。
        let cfg = Config::default();
        let lines = vec!["abcdef".to_string(), "XYZ".to_string()];
        // 1 本目 gap_top=10, course_idx 0 → "abcdef"。
        let mut g = Game::with_course(Config::default(), 1, vec![10u16, 5]);
        g.flap();
        let center = Config::default().rows / 2;
        // 1 本目が画面内（col<=50）に来るまで hover で進める（この時点では棒は 1 本）。
        for _ in 0..200 {
            if g.bird_cell().1 >= center {
                g.flap();
            }
            g.tick();
            if g.pipes()[0].x.round() as usize <= 50 {
                break;
            }
        }
        let p = &g.pipes()[0];
        assert_eq!(p.course_idx, 0);
        let col = p.x.round() as usize;
        let gap_top = p.gap_top;
        assert_ne!(col, 12, "test assumes pipe column differs from bird column");

        let frame = render(&g, Some(&lines));
        let glyphs: Vec<char> = "abcdef".chars().collect();
        for row in 1..(cfg.rows as i32 - 1) {
            let cell = frame.chars[row as usize][col];
            if pipe_blocks_row(gap_top, cfg.pipe_gap, cfg.rows, row) {
                assert_eq!(
                    cell,
                    glyphs[(row as usize - 1) % glyphs.len()],
                    "row {row}: wall must show the line's char (cycled)"
                );
                assert!(
                    frame.paint[row as usize][col] == Paint::Pipe,
                    "row {row}: wall cell must be Paint::Pipe"
                );
            } else {
                assert_eq!(cell, ' ', "row {row}: gap must be blank");
            }
        }
        assert!(
            !frame
                .chars
                .iter()
                .flatten()
                .any(|&c| ('\u{2800}'..='\u{28FF}').contains(&c)),
            "course mode must not render Braille pipes"
        );
    }

    #[test]
    fn course_mode_cycles_lines_across_pipes() {
        // 複数の棒が同時に画面内に出るとき、各棒が course_idx に応じて別の行の文字を描く
        // （course[0]→lines[0]="A...", course[1]→lines[1]="B..."）。cycle 選択の統合検証。
        let cfg = Config::default();
        let lines = vec!["AAAAAAAA".to_string(), "BBBBBBBB".to_string()];
        // gap_top 9/10 はどちらも中央行(12)を含む隙間 → hover で生き延び 2 本目まで出せる。
        let mut g = Game::with_course(Config::default(), 1, vec![9u16, 10]);
        g.flap();
        let center = cfg.rows / 2;
        let mut idx0_col = None;
        let mut idx1_col = None;
        for _ in 0..400 {
            if g.bird_cell().1 >= center {
                g.flap();
            }
            g.tick();
            if g.phase() != Phase::Playing {
                break;
            }
            // 画面内の棒を course_idx 別に拾う（同 idx が複数あれば最後のもの）。
            idx0_col = None;
            idx1_col = None;
            for p in g.pipes() {
                let col = p.x.round();
                if col < 0.0 || col as usize >= cfg.cols as usize {
                    continue;
                }
                match p.course_idx {
                    0 => idx0_col = Some(col as usize),
                    1 => idx1_col = Some(col as usize),
                    _ => {}
                }
            }
            if idx0_col.is_some() && idx1_col.is_some() {
                break;
            }
        }
        let c0 = idx0_col.expect("a pipe from lines[0] should be on screen");
        let c1 = idx1_col.expect("a pipe from lines[1] should be on screen");
        assert_ne!(c0, c1, "the two pipes must occupy different columns");

        // row 1 は gap_top(9/10) の上＝必ず塞ぐ行。各棒が対応する行の先頭文字を描く。
        let frame = render(&g, Some(&lines));
        assert_eq!(frame.chars[1][c0], 'A', "lines[0] pipe must render 'A'");
        assert_eq!(frame.chars[1][c1], 'B', "lines[1] pipe must render 'B'");
    }

    #[test]
    fn pipe_visual_cells_always_cover_collision_cell() {
        // 棒の描画 dot（round(2x) と +1）が属するセル集合は衝突セル round(x) を常に含む
        // ＝「見た目で触れていないのに死ぬ」ことはない（横サブセル化の安全性の根拠）。
        for i in 0..=400 {
            let x = i as f32 * 0.05; // 0.00..20.00 を 1/20 セル刻みで走査。
            let dx0 = (x * 2.0).round() as i32;
            let cells = [dx0.div_euclid(2), (dx0 + 1).div_euclid(2)];
            assert!(
                cells.contains(&(x.round() as i32)),
                "x={x}: visual cells {cells:?} must cover collision cell {}",
                x.round() as i32
            );
        }
    }
}
