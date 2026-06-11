//! core の状態を 1 フレームへ変換する純粋関数。
//!
//! 動く要素（鳥・棒）は Braille（1 セル = 2×4 ドット）でサブセル描画し、横 2 倍・縦 4 倍の
//! 実効解像度で滑らかに動かす。テキスト（HUD/天井・地面ライン/メッセージ/GameOver ボックス）は
//! 従来どおり文字で上書きする。色は付けない（描画時に [`Paint`] タグから付与する）。これにより
//! 判定（core の占有述語）と描画が同じセル定義を共有しつつ、ゴールデンテストが ANSI に汚されず
//! 安定する。

use flappy_core::{pipe_blocks_row, Game, Phase};

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
pub fn render(game: &Game) -> Frame {
    let cfg = game.config();
    let (cols, rows) = (cfg.cols as usize, cfg.rows as usize);

    // ドットビットマップ（rows*4 × cols*2）。動く要素をここに点で打つ。
    let (dot_w, dot_h) = (cols * 2, rows * 4);
    let mut dots = vec![vec![false; dot_w]; dot_h];

    // 棒: dot-x = round(x*2) から幅 2 ドット（=1 セル幅）。塞ぐセル行の dot-y 4 本を立てる。
    // 横は dot 解像度（既知の制約: 衝突は round(p.x) の 1 セルで判定されるため視覚との差は
    // 最大 1/4 セル。描画セルは衝突セル round(p.x) を常に含むので「触れて見えないのに死ぬ」
    // ことはなく、ずれは「触れて見えても生きている」側にだけ倒れる）。
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

    // 鳥: dot-x = bird_col*2 から幅 2 ドット、dot-y 中心から縦 2 ドットのブロブ。
    // 縦は dot 解像度（既知の制約: 衝突は bird_cell() の round 行で判定されるため、視覚位置と
    // 衝突行は最大 0.5 セルずれうる。視覚は真の f32 位置に忠実）。
    // GameOver は ✕ を文字で出すためブロブは描かない（overlay_text 参照）。
    if game.phase() != Phase::GameOver {
        let bird_dx0 = (cfg.bird_col * 2.0).round() as i32;
        let bird_dy_center = (game.bird_y() * 4.0).round() as i32;
        for dx in [bird_dx0, bird_dx0 + 1] {
            if dx < 0 || dx as usize >= dot_w {
                continue;
            }
            for dy in [bird_dy_center, bird_dy_center + 1] {
                if dy >= 0 && (dy as usize) < dot_h {
                    dots[dy as usize][dx as usize] = true;
                }
            }
        }
    }

    // ドットを 2×4 ブロックごとに Braille へパック。立っているセルに塗り分けを付ける。
    let mut chars = vec![vec![' '; cols]; rows];
    let mut paint = vec![vec![Paint::None; cols]; rows];
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
            chars[r][c] = braille(mask);
            if mask != 0 {
                // 鳥セルは鳥列・鳥行帯のとき Bird、それ以外（立っている＝棒）は Pipe。
                paint[r][c] = paint_for_cell(game, r, c);
            }
        }
    }

    // テキストレイヤーを文字で上書き（paint は None に戻す）。
    overlay_text(game, &mut chars, &mut paint, cfg.cols, cfg.rows);

    Frame { chars, paint }
}

/// 立っているセルの塗り分けを決める。鳥セル（鳥列かつ鳥の縦 2 ドット帯に重なる行）は Bird、
/// それ以外は Pipe。GameOver はブロブを描かないため常に Pipe。
fn paint_for_cell(game: &Game, r: usize, c: usize) -> Paint {
    if game.phase() == Phase::GameOver {
        return Paint::Pipe;
    }
    let cfg = game.config();
    let bird_c = cfg.bird_col.round() as i32;
    let bird_dy_center = (game.bird_y() * 4.0).round() as i32;
    // 鳥ブロブが占める行（dy_center と dy_center+1 が属するセル行）。
    let bird_rows = [bird_dy_center / 4, (bird_dy_center + 1) / 4];
    if c as i32 == bird_c && bird_rows.contains(&(r as i32)) {
        Paint::Bird
    } else {
        Paint::Pipe
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
    let score_text = format!("SCORE {}", game.score);
    place_at(&mut chars[0], &mut paint[0], &score_text, 1);
    let best_text = format!("BEST {}", game.best);
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
            place_centered(&mut chars[3], &mut paint[3], "F L A P P Y", cols);
            place_centered(&mut chars[8], &mut paint[8], "──  press SPACE  ──", cols);
        }
        Phase::GameOver => {
            draw_gameover_box(chars, paint, cols, game.score);
            // 死亡した鳥は ✕ の文字で表す（render はブロブを描かない）。棒セルの上で
            // 死んだ場合も ✕ が棒色にならないよう paint を BirdDead にする（赤で描く）。
            let (bc, br) = game.bird_cell();
            if (br as usize) < rows as usize && bc < cols {
                chars[br as usize][bc as usize] = BIRD_DEAD;
                paint[br as usize][bc as usize] = Paint::BirdDead;
            }
        }
        Phase::Playing => {}
    }
}

/// 既知状態の core を 1 フレームの文字グリッドへ変換する（プレーンテキスト、テスト/ゴールデン用）。
#[cfg(test)]
pub fn scene_to_string(game: &Game) -> String {
    render(game).to_text()
}

/// GameOver の罫線ボックスを中央へ描く（内側幅 20）。
fn draw_gameover_box(chars: &mut [Vec<char>], paint: &mut [Vec<Paint>], cols: u16, score: u32) {
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

    /// Braille 範囲（U+2800..=U+28FF）の非空グリフか。
    fn is_braille(ch: char) -> bool {
        ('\u{2801}'..='\u{28FF}').contains(&ch)
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
        // 鳥は col 12・bird_y=12.0 → dot-y 中心 48 → 48/4=12 行目。Braille グリフで描かれる。
        let bird = ls[12].chars().nth(12).unwrap();
        assert!(
            is_braille(bird),
            "bird cell should be a non-empty Braille glyph, got {bird:?}"
        );
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
        assert!(is_braille(bird), "bird should remain a Braille glyph");
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
            let frame = render(&g);
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
        assert!(scene.contains("GAME  OVER"));
        assert!(scene.contains("SPACE / r : retry"));
        assert!(scene.contains("q         : quit"));
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
        let frame = render(&g);
        assert!(
            frame.paint.iter().flatten().any(|p| *p == Paint::BirdDead),
            "dead bird cell should be tagged Paint::BirdDead"
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
    fn bird_subcell_glyph_changes_as_bird_drifts() {
        // bird_y の端数（縦ドット位置）が変わると同じ行内でも Braille グリフが変わる
        // ＝サブセル化の本質。flap の上昇〜重力で戻る間、鳥が row 12 帯に居るうちの
        // グリフを集める。
        let mut g = Game::new(Config::default(), 1);
        g.flap(); // 上向き初速で上昇開始（その後重力で減速し row 12 帯へ戻る）。
        let mut glyphs = std::collections::HashSet::new();
        // 初期状態（bird_y=12.0）。
        glyphs.insert(render(&g).chars[12][12]);
        for _ in 0..30 {
            g.tick();
            // 鳥がまだ row 12 にいる間だけ収集（離れたら端数比較にならない）。
            if g.bird_cell().1 == 12 {
                glyphs.insert(render(&g).chars[12][12]);
            }
        }
        glyphs.remove(&' ');
        assert!(
            glyphs.iter().all(|&c| is_braille(c)),
            "all bird glyphs in row 12 should be Braille"
        );
        assert!(
            glyphs.len() >= 2,
            "sub-cell drift within a row should yield >= 2 distinct glyphs, got {glyphs:?}"
        );
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
