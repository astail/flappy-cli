//! flappy-web — web-sys で canvas 描画する薄いレンダラ（wasm32 / trunk）。
//!
//! 描画 (#16): core の状態を canvas に矩形で描画する。term の `scene_to_string` と
//! 同じ「core グリッドをなぞって塗る」構造で、占有述語 [`pipe_blocks_row`] と鳥セル
//! ゲッタを判定と共有する（描画と判定の乖離を防ぐ）。
//! 入力 (#17): Space/click/tap を core 操作へルーティング（GameOver なら `restart()`、
//! 他は `flap()` ＝ term の `route` と同一）。Space はページスクロール抑止のため
//! prevent_default する。固定 DT の物理更新・visibilitychange 対応は #18 で追加する。

use std::cell::RefCell;
use std::rc::Rc;

use flappy_core::{pipe_blocks_row, Config, Game, Phase};
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

/// 主操作（Space/click/tap）を core へ振り分ける（term の `input::route` と同一）。
/// GameOver はリスタート、それ以外はフラップ（Ready は初回フラップで Playing 化）。
fn apply_primary(game: &mut Game) {
    if game.phase() == Phase::GameOver {
        game.restart();
    } else {
        game.flap();
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
    ctx.set_fill_style_str(COLOR_PIPE);
    for p in game.pipes() {
        let c = p.x.round() as i32;
        if c >= 0 && (c as u16) < cols {
            for row in 0..rows as i32 {
                if pipe_blocks_row(p.gap_top, cfg.pipe_gap, rows, row) {
                    ctx.fill_rect(c as f64 * cell, row as f64 * cell, cell, cell);
                }
            }
        }
    }

    // 地面ライン（最下行の横帯）。
    ctx.set_fill_style_str(COLOR_GROUND);
    ctx.fill_rect(0.0, (rows as f64 - 1.0) * cell, w, cell);

    // 鳥（塗り円。衝突と同じ丸めのセル）。GameOver は赤。
    let (bc, br) = game.bird_cell();
    let cx = (bc as f64 + 0.5) * cell;
    let cy = (br as f64 + 0.5) * cell;
    let r = cell * 0.5 - 1.0;
    ctx.set_fill_style_str(if game.phase() == Phase::GameOver {
        COLOR_BIRD_DEAD
    } else {
        COLOR_BIRD
    });
    ctx.begin_path();
    let _ = ctx.arc(cx, cy, r, 0.0, std::f64::consts::PI * 2.0);
    ctx.fill();

    // HUD（最上行）: 左 SCORE、右 BEST。
    ctx.set_fill_style_str(COLOR_TEXT);
    ctx.set_font("16px monospace");
    ctx.set_text_baseline("middle");
    let hud_y = cell * 0.5;
    ctx.set_text_align("left");
    let _ = ctx.fill_text(&format!("SCORE {}", game.score), cell, hud_y);
    ctx.set_text_align("right");
    let _ = ctx.fill_text(&format!("BEST {}", game.best), w - cell, hud_y);

    // メッセージのオーバーレイ（term の行配置に合わせる）。
    ctx.set_text_align("center");
    match game.phase() {
        Phase::Ready => {
            ctx.set_font("bold 32px monospace");
            let _ = ctx.fill_text("F L A P P Y", w / 2.0, 3.5 * cell);
            ctx.set_font("16px monospace");
            let _ = ctx.fill_text("──  press SPACE  ──", w / 2.0, 8.5 * cell);
        }
        Phase::GameOver => {
            ctx.set_font("bold 24px monospace");
            let _ = ctx.fill_text("GAME  OVER", w / 2.0, 3.5 * cell);
            ctx.set_font("16px monospace");
            let _ = ctx.fill_text(&format!("SCORE {}", game.score), w / 2.0, 5.5 * cell);
            let _ = ctx.fill_text("SPACE / r : retry", w / 2.0, 7.0 * cell);
            let _ = ctx.fill_text("q : quit", w / 2.0, 8.0 * cell);
        }
        Phase::Playing => {}
    }
}

fn main() {
    let document = window().document().expect("no document");
    let canvas = document
        .get_element_by_id("canvas")
        .expect("missing #canvas")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#canvas is not a canvas");

    // canvas = 64*16 × 24*16（論理グリッドは core が一意の真実）。
    // game は RAF 描画と入力ハンドラで共有するため Rc<RefCell> で包む（JS は単一
    // スレッドなので両者の borrow は実行時に重ならない）。
    let game = Rc::new(RefCell::new(Game::new(Config::default(), 1)));
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

    // RAF ループ。同一の FnMut クロージャを毎フレーム再予約し続ける
    // （FnOnce を毎回 drop する方式の実行中 drop 問題を避ける正準パターン）。
    // #17 は入力を core へ反映するのみ。物理（tick）は未配線なので flap しても鳥は
    // 落下せず、Ready→Playing でメッセージが消える反応までを描画する（#18 で物理）。
    let f: Rc<RefCell<Option<RafCallback>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move |_time: f64| {
        draw(&ctx, &game.borrow());
        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut(f64)>));
    request_animation_frame(g.borrow().as_ref().unwrap());

    // クロージャを永続化（drop されると RAF が止まる）。
    std::mem::forget(g);
}
