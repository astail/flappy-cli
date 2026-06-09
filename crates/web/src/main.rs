//! flappy-web — web-sys で canvas 描画する薄いレンダラ（wasm32 / trunk）。
//!
//! 本 issue (#15) ではクレート骨格として canvas を取得し、空の
//! requestAnimationFrame ループを回すところまでを用意する。固定 DT の物理更新・
//! 入力・矩形描画・visibilitychange 対応は後続 issue (#16〜#18) で追加する。

use std::cell::RefCell;
use std::rc::Rc;

use flappy_core::Config;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

/// 1 セルのピクセル幅（§5: 1セル=固定 px）。
const CELL: u32 = 16;

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

fn main() {
    let document = window().document().expect("no document");
    let canvas = document
        .get_element_by_id("canvas")
        .expect("missing #canvas")
        .dyn_into::<HtmlCanvasElement>()
        .expect("#canvas is not a canvas");

    // canvas = 64*16 × 24*16（論理グリッドは core が一意の真実）。
    let cfg = Config::default();
    canvas.set_width(cfg.cols as u32 * CELL);
    canvas.set_height(cfg.rows as u32 * CELL);

    // 2D コンテキストは取得だけ確認（描画は #16）。
    let _ctx = canvas
        .get_context("2d")
        .expect("get_context failed")
        .expect("no 2d context")
        .dyn_into::<CanvasRenderingContext2d>()
        .expect("not a 2d context");

    // 空の RAF ループ。同一の FnMut クロージャを毎フレーム再予約し続ける
    // （FnOnce を毎回 drop する方式の実行中 drop 問題を避ける正準パターン）。
    let f: Rc<RefCell<Option<RafCallback>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    *g.borrow_mut() = Some(Closure::wrap(Box::new(move |_time: f64| {
        // #15 は描画なし。次フレームを予約してループを継続する。
        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut(f64)>));
    request_animation_frame(g.borrow().as_ref().unwrap());

    // クロージャを永続化（drop されると RAF が止まる）。
    std::mem::forget(g);
}
