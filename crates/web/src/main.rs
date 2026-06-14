//! flappy-web — web-sys で canvas 描画する薄いレンダラ（wasm32 / trunk）。
//!
//! 描画 (#16): core の状態を canvas に矩形で描画する。term の `scene_to_string` と
//! 同じ「core グリッドをなぞって塗る」構造で、占有述語 [`pipe_blocks_row`] と鳥セル
//! ゲッタを判定と共有する（描画と判定の乖離を防ぐ）。
//! 入力 (#17): Space/click/tap を core 操作へルーティング（GameOver なら `restart()`、
//! 他は `flap()` ＝ term の `route` と同一）。Space はページスクロール抑止のため
//! prevent_default する。
//! ループ (#18): RAF で実時間を蓄積し固定 [`DT`] 刻みで `tick()`（1 フレーム上限
//! 0.10s、term と共通の蓄積ループ＝描画頻度非依存で決定論）。`visibilitychange` で
//! 復帰時にアキュムレータをリセットし、長時間バックグラウンド後の一発死を防ぐ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flappy_core::{
    pipe_blocks_row, primary_action, Accumulator, Config, Game, Phase, PrimaryAction,
    GAMEOVER_RETRY_HINT, GAMEOVER_TITLE, READY_HINT, READY_TITLE, VERSION,
};
use gloo_events::{EventListener, EventListenerOptions};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent};

/// 1 セルのピクセル幅（§5: 1セル=固定 px）。
const CELL: u32 = 16;

// 色は term と揃える（恐竜風の淡背景＋濃色要素、棒は緑）。§2 要素対応表。
const COLOR_BG: &str = "#e8f4f8"; // 淡背景（index.html の body と同色）
const COLOR_PIPE: &str = "#2e8b2e"; // 棒（緑）
const COLOR_GROUND: &str = "#888888"; // 地面ライン
const COLOR_BIRD: &str = "#333333"; // 鳥（生存）
const COLOR_BIRD_DEAD: &str = "#c0392b"; // 鳥（GameOver）
const COLOR_TEXT: &str = "#333333"; // HUD / メッセージ

/// RAF コールバック（timestamp を受ける FnMut）。
type RafCallback = Closure<dyn FnMut(f64)>;

fn window() -> web_sys::Window {
    web_sys::window().expect("no global window")
}

fn request_animation_frame(f: &RafCallback) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("request_animation_frame failed");
}

/// 主操作（Space/click/tap）を core へ振り分ける。phase→効果の判定は core の
/// [`primary_action`] が単一ソース（#137: term の `input::route` と判定を共有）。
/// 主操作の分類（どの JS イベントを主操作とみなすか）は web の責務として残す。
fn apply_primary(game: &mut Game) {
    match primary_action(game.phase()) {
        PrimaryAction::Flap => game.flap(),
        PrimaryAction::Restart => game.restart(),
    }
}

/// 1 フレームを canvas に矩形で描画する。term の `scene_to_string` と同じ順序・同じ
/// 占有述語を使い、両プラットフォームで見た目を揃える。
fn draw(ctx: &CanvasRenderingContext2d, game: &Game) {
    let cfg = game.config();
    let (cols, rows) = (cfg.cols, cfg.rows);
    let cell = CELL as f64;
    let w = cols as f64 * cell;
    let h = rows as f64 * cell;

    // 背景（淡色で全面クリア）。
    ctx.set_fill_style_str(COLOR_BG);
    ctx.fill_rect(0.0, 0.0, w, h);

    // 棒（緑）。占有述語を判定と共有し、棒セルだけを塗る。
    // x は term の dot-x と同じ 1/2 セル刻み（(x*2).round()/2）で量子化し、スクロールの段差を
    // term と揃える（CLAUDE.md: term/web は同じ作りに揃える）。幅は 1 セル、端は canvas が
    // クリップする（term の dot 単位クリップと等価）。
    // 既知の制約（term #71 と共通）: 衝突は core の round セル（p.x.round()）で判定するため
    // 視覚位置との差は最大 1/4 セル。描画セルは衝突セルを常に含むので「触れて見えないのに死ぬ」
    // ことはない（term 側にテスト pipe_visual_cells_always_cover_collision_cell）。
    ctx.set_fill_style_str(COLOR_PIPE);
    for p in game.pipes() {
        let x = (p.x * 2.0).round() / 2.0;
        let px = x as f64 * cell;
        if px + cell > 0.0 && px < w {
            for row in 0..rows as i32 {
                if pipe_blocks_row(p.gap_top, cfg.pipe_gap, rows, row) {
                    ctx.fill_rect(px, row as f64 * cell, cell, cell);
                }
            }
        }
    }

    // 天井ライン（最上行の横帯）と地面ライン（最下行の横帯）。
    ctx.set_fill_style_str(COLOR_GROUND);
    ctx.fill_rect(0.0, 0.0, w, cell);
    ctx.fill_rect(0.0, (rows as f64 - 1.0) * cell, w, cell);

    // 鳥（塗り円）。横は bird_col 固定。縦は生存・死亡とも描画用セル bird_display_cell() の
    // round 行のセル中心に置く。term は鳥を ● / ✕ の 1 文字（= 1 セル）で描くため、
    // web も「1 セル = 1 円」を同じ round 行に合わせる（term/web で見た目・段差を一致させる）。
    // 天井死で衝突用 row が 0 に来ても、core の bird_display_cell が row 1（プレイエリア最上行）へ
    // クランプし、円が天井ライン/HUD 帯（row 0）を潰さない（#138: クランプは core 単一ソース・term と一致）。
    let cx = (cfg.bird_col as f64 + 0.5) * cell;
    let dead = game.phase() == Phase::GameOver;
    let cy = (game.bird_display_cell().1 as f64 + 0.5) * cell;
    let r = cell * 0.5 - 1.0;
    ctx.set_fill_style_str(if dead { COLOR_BIRD_DEAD } else { COLOR_BIRD });
    ctx.begin_path();
    let _ = ctx.arc(cx, cy, r, 0.0, std::f64::consts::PI * 2.0);
    ctx.fill();

    // HUD（最上行）: 左 SCORE、右 BEST。
    ctx.set_fill_style_str(COLOR_TEXT);
    ctx.set_font("16px monospace");
    ctx.set_text_baseline("middle");
    let hud_y = cell * 0.5;
    ctx.set_text_align("left");
    let _ = ctx.fill_text(&format!("SCORE {}", game.score()), cell, hud_y);
    ctx.set_text_align("right");
    let _ = ctx.fill_text(&format!("BEST {}", game.best()), w - cell, hud_y);

    // version（地面ライン右端に控えめに）。term の scene と同じ右下配置・単一ソース。
    ctx.set_font("12px monospace");
    let _ = ctx.fill_text(
        &format!("v{VERSION}"),
        w - cell * 0.5,
        (rows as f64 - 0.5) * cell,
    );

    // メッセージのオーバーレイ（term の行配置に合わせる）。
    ctx.set_text_align("center");
    match game.phase() {
        Phase::Ready => {
            ctx.set_font("bold 32px monospace");
            let _ = ctx.fill_text(READY_TITLE, w / 2.0, 3.5 * cell);
            ctx.set_font("16px monospace");
            let _ = ctx.fill_text(READY_HINT, w / 2.0, 8.5 * cell);
        }
        Phase::GameOver => {
            // 罫線ボックス相当の枠（#76: term の draw_gameover_box と同じ行・同じ幅）。
            // term はボックス文字で背面の棒を隠すため、web も内側を背景色で塗ってから枠線を引く。
            let box_w_cells = (GAMEOVER_RETRY_HINT.chars().count() + 2) as f64; // 罫線込み幅（セル）
            let bx = ((cols as f64 - box_w_cells) / 2.0).floor() * cell;
            let by = 2.0 * cell;
            let (bw, bh) = (box_w_cells * cell, 6.0 * cell);
            ctx.set_fill_style_str(COLOR_BG);
            ctx.fill_rect(bx, by, bw, bh);
            ctx.set_stroke_style_str(COLOR_TEXT);
            ctx.stroke_rect(bx, by, bw, bh);
            // 文言は core の定数（term と同一ソース）。行位置も term のボックス内行に合わせ、
            // x は枠の中心（キャンバス中心とは 0.5 セルずれる）に揃える。
            let box_cx = bx + bw / 2.0;
            ctx.set_fill_style_str(COLOR_TEXT);
            ctx.set_font("bold 16px monospace");
            let _ = ctx.fill_text(GAMEOVER_TITLE, box_cx, 3.5 * cell);
            ctx.set_font("16px monospace");
            let _ = ctx.fill_text(&format!("SCORE {}", game.score()), box_cx, 4.5 * cell);
            let _ = ctx.fill_text(GAMEOVER_RETRY_HINT, box_cx, 5.5 * cell);
            // 「q : quit」行は term のみ（web に終了概念がないため。DESIGN §2 の許容差）。
        }
        Phase::Playing => {}
    }
}

fn main() {
    // wasm の panic を JS コンソールに Rust のメッセージ・位置情報付きで出す。
    console_error_panic_hook::set_once();

    let document = window().document().expect("no document");
    let canvas = document
        .get_element_by_id("canvas")
        .expect("missing #canvas")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#canvas is not a canvas");

    // canvas = 64*16 × 24*16（論理グリッドは core が一意の真実）。
    // game は RAF 描画と入力ハンドラで共有するため Rc<RefCell> で包む（JS は単一
    // スレッドなので両者の borrow は実行時に重ならない）。
    let game = Rc::new(RefCell::new(Game::new(
        Config::default(),
        js_sys::Date::now() as u64,
    )));
    {
        let cfg = game.borrow();
        let cfg = cfg.config();
        canvas.set_width(cfg.cols as u32 * CELL);
        canvas.set_height(cfg.rows as u32 * CELL);
    }

    let ctx = canvas
        .get_context("2d")
        .expect("get_context failed")
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .expect("not a 2d context");

    // 入力リスナ（#17）。Space/click/tap を主操作へルーティング。
    // - keydown: Space のみ。リピート（押しっぱなし）は flap せず、ページスクロール
    //   抑止のため prevent_default のみ行う。prevent_default を効かせるには passive
    //   でないリスナが必要なので enable_prevent_default で登録する（DESIGN §5）。
    // - click / touchstart: どこでも主操作。touchstart は prevent_default で
    //   合成クリックの二重発火とスクロール/ズームを抑止する。
    let prevent = EventListenerOptions::enable_prevent_default();
    {
        let game = game.clone();
        EventListener::new_with_options(&window(), "keydown", prevent, move |event| {
            let ev = event.dyn_ref::<KeyboardEvent>().unwrap();
            if ev.key() == " " {
                ev.prevent_default();
                if !ev.repeat() {
                    apply_primary(&mut game.borrow_mut());
                }
            } else if ev.key() == "r" || ev.key() == "R" {
                // term の Input::Restart と同じく phase 非依存で restart。
                if !ev.repeat() {
                    game.borrow_mut().restart();
                }
            }
        })
        .forget();
    }
    {
        let game = game.clone();
        EventListener::new(&window(), "click", move |_event| {
            apply_primary(&mut game.borrow_mut());
        })
        .forget();
    }
    {
        let game = game.clone();
        EventListener::new_with_options(&window(), "touchstart", prevent, move |event| {
            event.prevent_default();
            apply_primary(&mut game.borrow_mut());
        })
        .forget();
    }

    // 蓄積ループの状態。RAF 描画と visibilitychange ハンドラで共有する。
    // last_time: 前フレームの RAF タイムスタンプ（ms）。None は「次フレームを基準に
    // やり直す」合図（初回・復帰直後）。acc: core の Accumulator（未消化時間＋固定ステップ消化）。
    let acc = Rc::new(RefCell::new(Accumulator::new()));
    let last_time = Rc::new(Cell::new(None::<f64>));

    // visibilitychange: 非表示→復帰の一発死防止。背景タブでは RAF 自体が止まるため、
    // 復帰時にアキュムレータと前回時刻をリセットして溜まった実時間を捨てる（DESIGN §5）。
    {
        let acc = acc.clone();
        let last_time = last_time.clone();
        EventListener::new(&document, "visibilitychange", move |_event| {
            *acc.borrow_mut() = Accumulator::new();
            last_time.set(None);
        })
        .forget();
    }

    // RAF ループ。同一の FnMut クロージャを毎フレーム再予約し続ける
    // （FnOnce を毎回 drop する方式の実行中 drop 問題を避ける正準パターン）。
    // 前フレームからの実時間を蓄積し、固定 DT 刻みで tick（描画頻度非依存＝決定論）。
    let f: Rc<RefCell<Option<RafCallback>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move |time: f64| {
        // 実経過時間を core の Accumulator に渡し、消化すべき tick 数を得る。
        // MAX_FRAME_DT クランプ（spiral of death 防止）と固定ステップ消化は core が単一ソース（#139）。
        let ticks = match last_time.get() {
            Some(last) => acc.borrow_mut().advance(((time - last) / 1000.0) as f32),
            None => 0, // 初回・復帰直後は基準フレームを置くだけ（tick しない）。
        };
        last_time.set(Some(time));

        {
            let mut game = game.borrow_mut();
            for _ in 0..ticks {
                game.tick();
            }
        }

        draw(&ctx, &game.borrow());
        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut(f64)>));
    request_animation_frame(g.borrow().as_ref().unwrap());

    // クロージャを永続化（drop されると RAF が止まる）。
    std::mem::forget(g);
}
