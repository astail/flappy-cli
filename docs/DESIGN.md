# Flappy CLI — 設計ドキュメント

ターミナル（mac/Linux）とブラウザの両方で動く Flappy Bird 系のドットゲーム。
自キャラはドット1個。スペースで上昇し、重力で落下。画面は自動で横スクロールし、上下の棒の隙間を抜けていく。見た目は Chrome の恐竜ゲーム風のシンプルなドット絵。

## 概要

| 項目 | 決定 |
|---|---|
| 言語/技術 | **Rust + WASM**（CLI は単一バイナリ、ブラウザは wasm にコンパイル） |
| ゲーム形式 | **エンドレス**（棒を抜けるごとに +1 点、終わりなし、最高スコアを競う） |
| ブラウザ配信 | **静的ビルド+配信可**（`trunk build` で静的ファイル化 → GitHub Pages 等に配信可能） |

### 設計の核

ゲームロジックを **I/O を一切持たない純粋な core クレート**に閉じ込め、ターミナルとブラウザは「core の状態を描画し、入力を core に渡すだけ」の薄いレンダラにする。これで Rust 1 言語・ロジック完全共有を実現する。

```
flappy-cli/
├── Cargo.toml              # workspace
├── rust-toolchain.toml     # wasm32 target を含める
├── crates/
│   ├── core/               # flappy-core: 純粋なゲームロジック（依存ゼロ）
│   │   └── src/lib.rs      #   + rng.rs (SplitMix64)
│   ├── term/               # flappy-term: crossterm でターミナル描画（bin名 `flappy`）
│   │   └── src/main.rs
│   └── web/                # flappy-web: web-sys で canvas 描画（wasm binary）
│       ├── index.html      #   trunk のエントリ
│       └── src/main.rs
└── .github/workflows/      # ci.yml / pages.yml / audit.yml / release.yml（§10）
```

---

## 1. ゲーム全体フロー（状態遷移 & ループ）

### 状態遷移

3状態のシンプルな state machine。core の `Phase` がそのまま対応する。

```
                   SPACE                衝突（棒 / 天井 / 地面）
   start ─▶ ┌───────┐ ─────────▶ ┌─────────┐ ─────────────────▶ ┌──────────┐
            │ Ready │            │ Playing │                    │ GameOver │
            └───────┘            └─────────┘                    └──────────┘
                ▲                                                   │
                └───────────────── SPACE (restart) ─────────────────┘

   q / Esc（term）・ウィンドウを閉じる（web） … いつでも終了
```

### 入力 → 効果

| 状態 | キー / 操作 | 効果 |
|---|---|---|
| Ready | SPACE / クリック・タップ | ゲーム開始（→Playing、初回フラップ込み） |
| Playing | SPACE / クリック・タップ | フラップ（上昇） |
| Playing | （放置） | 重力で落下し続ける |
| GameOver | SPACE / クリック | リスタート（→Ready） |
| 全状態 | r | リスタート（→Ready。phase 非依存、term/web 共通） |
| 全状態 | q / Esc（term のみ） | 終了 |

web の Space は `preventDefault` でページスクロールを抑止する。

Space 押しっぱなし（キーリピート）時の連続フラップは term/web で挙動が異なる（**意図的な許容差**）: web は `keydown` の `event.repeat` を無視して 1 回だけフラップするが、term は raw mode の標準入力では端末のオートリピートと新規打鍵を区別できないため連続フラップになる（kitty keyboard protocol は未使用方針）。

### ゲームループ（term / web 共通の流れ）

物理は**固定タイムステップ**で進む。両プラットフォームは「実時間を蓄積し、固定 `DT` 刻みで core を進め、状態を描画する」だけ（描画頻度に依存しない＝term/web で同一挙動・決定論）。

```
const DT = 1.0 / 60.0                 // 固定物理ステップ（core が公開）
last = now();  acc = 0.0
loop {
    t = now();  acc += min(t - last, 0.10);  last = t   // 実時間を蓄積。1フレーム上限 0.10s=6tick（超過分は捨てる＝処理が遅れたら物理がスロー化し spiral of death を防止。無操作落下も最大~3行に制限）
    for ev in poll_input() {              // 非ブロッキング入力
        Space/Click => if phase == GameOver { game.restart() } else { game.flap() }
        R           => game.restart()      //   全 phase で即リスタート（term/web 共通）
        Q/Esc       => quit                //   term のみ
    }
    while acc >= DT { game.tick(); acc -= DT }     // 固定ステップで物理更新（tick は内部で DT を使う。Playing 時のみ）
    render(&game)                          // core の状態 → 文字グリッド / canvas 矩形
    // 描画頻度は自由（term ~30–60Hz / web は requestAnimationFrame）。物理は常に DT 刻み
}
```

---

## 2. 画面レイアウト

論理グリッドは **64列 × 24行**（両プラットフォーム共通＝同じ画面）。下記は縮小イメージ（_not to scale_。鳥は実際は col 12、実座標は §3 座標系 / §7 を参照）。図中の `●` / `█` は概念表現で、実際の動く要素（鳥・棒）は Braille サブセル（1 セル = 横2×縦4 ドット）で滑らかに描画される（§4 参照）。
最上行を HUD（SCORE / BEST）、最下行付近を地面ラインにし、中央の広い帯がプレイエリア。

### Ready（開始前）

```
 SCORE 0                          BEST 0
                                          
                                          
                F L A P P Y               
                                          
      ●                                   
                                          
                                          
            ──  press SPACE  ──           
                                          
──────────────────────────────────────────
```

### Playing（プレイ中）

鳥は固定列（col 12）で上下し、棒が右から左へ流れる。隙間 `pipe_gap` を抜けるたびに SCORE +1。

```
 SCORE 3                         BEST 12
           █                  █           
           █                  █           
           █                              
     ●                                    
                              █           
           █                  █           
           █                  █           
           █                  █           
──────────────────────────────────────────
```

### GameOver（衝突後）

```
 SCORE 7                         BEST 12
                                          
       ╔═════════════════════════╗        
       ║       GAME  OVER        ║        
       ║         SCORE 7         ║        
       ║SPACE / click / r : retry║        
    ✕  ║q                 : quit ║        
       ╚═════════════════════════╝        
──────────────────────────────────────────
```

文言（GAME  OVER / retry 案内）は `flappy-core` の定数（`GAMEOVER_TITLE` / `GAMEOVER_RETRY_HINT`）を term/web が共有する（文言ズレの再発防止）。`q : quit` 行だけは **term のみ**（web に終了概念がないため省略する。許容差）。web は同じ行・同じ幅の枠を `stroke_rect` で描く。

### 要素の対応（term ⇄ web で見た目を揃える）

| 要素 | term（文字） | web（canvas） |
|---|---|---|
| 鳥 | Braille サブセルの 2×2 ドットブロブ（GameOver 時は `✕`・赤） | 塗り円（GameOver 時は赤） |
| 棒 | Braille サブセル（緑、横 1/2 セル刻み） | 緑の矩形（同じ 1/2 セル刻み） |
| 地面ライン | `─` の横帯（最下行） | 同位置の横帯 |
| HUD（SCORE/BEST） | 最上行テキスト | 上部テキスト |
| メッセージ枠 | 罫線ボックス | canvas テキスト（必要なら枠） |
| 背景 | 端末既定（暗） | 淡色（恐竜ゲーム風） |

- グリッドより端末が広い場合はセンタリング（レターボックス）。**最小サイズ = 64×24**。未満なら「端末を 64×24 以上にしてください」と表示して描画を止める（ポーズ）。プレイ中の端末リサイズ（`Event::Resize`）は次フレームで再センタリングのみ行い、ゲーム状態は不変。
- web の canvas は 1セル=固定 px（例 16px）→ `64*16 × 24*16` を CSS で中央寄せ。

---

## 3. core クレート（flappy-core）— 依存ゼロ・純粋ロジック

**最重要。両プラットフォームが共有する唯一の真実。** I/O・描画・sleep・乱数エントロピー取得を一切持たない。`tick()` 駆動の決定論的な状態機械。

### 座標系
- 論理グリッド `W×H`（デフォルト **64列 × 24行**）。両プラットフォームで共通＝「同じ画面」。
- 鳥の x は固定列（col 12）。鳥の y は `f32`（行単位）。棒が左へスクロールする。
- ターミナルは 1セル→1文字、ブラウザは 1セル→Nピクセル(例 16px) にマップ。

### 型（スケッチ）
```rust
pub struct Config {            // チューニング値はここに集約
    pub cols: u16, pub rows: u16,
    pub bird_col: f32,
    pub gravity: f32,          // 行/秒^2
    pub flap_impulse: f32,     // 上向き初速（負値）
    pub scroll_speed: f32,     // 列/秒
    pub pipe_gap: u16,         // 隙間の縦幅（行）
    pub pipe_spacing: f32,     // 棒の間隔（列）
    pub vy_max: f32,           // 終端速度（下向き上限、行/秒）
}
pub enum Phase { Ready, Playing, GameOver }
pub struct Pipe { pub x: f32, pub gap_top: u16, pub passed: bool }
pub struct Game {
    cfg: Config, rng: Rng, phase: Phase,
    bird_y: f32, bird_vy: f32,
    pipes: Vec<Pipe>,
    dist_to_next: f32,         // 次の棒生成までの距離
    pub score: u32, pub best: u32,
}
```

### API（core が公開する最小操作）
- `Game::new(cfg, seed) -> Game`
- `flap(&mut self)` — Ready なら Playing 化、Playing なら `bird_vy = flap_impulse`
- `tick(&mut self)` — Playing 時のみ物理更新（後述）。**内部で固定 `DT = 1/60` を進める**（可変 dt は受け取らない＝決定論を型で強制。実 dt の揺れは物理に入らずプラットフォーム非依存）。core は `pub const DT: f32 = 1.0 / 60.0;` を公開し、レンダラ側がアキュムレータで `tick()` の呼び出し回数を制御する（§1）
- `restart(&mut self)` — best を保持して初期化
- 描画用ゲッター: `phase()`, `bird_cell() -> (u16,u16)`, `bird_y() -> f32`, `pipes()`, `config()`, `score`, `best`。`bird_cell()` は衝突と同じ round 済み行（判定と描画の一致用）、`bird_y()` はサブセル描画用の連続座標、という役割分担
- `pipe_blocks_row(gap_top, pipe_gap, rows, row) -> bool` — 棒がその行を占有するかの純粋述語。**判定と描画が共有する唯一の占有定義**（§3 冒頭の「描画と判定の乖離防止」の実体）
- 共有定数: `DT`（固定ステップ）, `VERSION`（HUD 表示用）, `GAMEOVER_TITLE` / `GAMEOVER_RETRY_HINT`（GameOver 画面文言。term/web で文言を共有）
- 初期化（new / restart）: `bird_y` は画面中央付近。最初の棒を `x = cols`（右端）に1本だけ生成し、`dist_to_next = pipe_spacing` から開始（1本目が鳥に届くまで約 `cols - bird_col` 列 ≈ 1画面ぶんの助走になり、開始即死を防ぐ）

### tick の中身（処理順）
セル化は描画と判定で**同じ丸めを使う**: `bird_row = bird_y.round() as i32`、固定列 `bird_c = bird_col as i32`、`pipe_col = p.x.round() as i32`。**棒は常に 1 列幅**（`pipe_width` は持たない）。境界は HUD=行0・地面ライン=行 `rows-1`（鳥が乗れる範囲 `1..=rows-2`）。棒の描画セルは衝突と**同一定義**: `1 ≤ row < gap_top` ∪ `gap_top + pipe_gap ≤ row ≤ rows-2`。描画と判定は同じ純粋関数を共有し、両者の乖離（「隙間を通ったのに死ぬ」系バグ）を防ぐ。

1. `bird_vy = (bird_vy + gravity * DT).min(vy_max); bird_y += bird_vy * DT`（`vy_max` で終端速度を制限）
2. 全 pipe の `x -= scroll_speed * DT`、画面外(`x < -1`)の pipe を除去
3. `dist_to_next -= scroll_speed * DT`、0 以下になったら `gap_top` を rng で `[1, rows-1-pipe_gap]`（**両端含む / inclusive**）から選び新 pipe を右端（`x = cols`）に生成、`dist_to_next += pipe_spacing`（剰余を保持し spacing が drift しない）
4. **衝突判定（先に評価。当たれば加点せず `phase = GameOver`）**
   - 天井/地面: `bird_row < 1 || bird_row >= rows - 1`
   - 棒: `pipe_col == bird_c` の pipe があり、かつ `bird_row < gap_top || bird_row >= gap_top + pipe_gap`
5. **スコア**: 棒が鳥を完全に通り抜けた（`pipe_col < bird_c`）未 passed pipe があれば `score += 1; passed = true`（best も更新）

> 衝突は移動後セルの離散判定で十分。**横**: 固定ステップなので棒の 1 フレーム移動は `scroll_speed × DT`。不変条件 **`scroll_speed × DT < 1.0`**（DT=1/60 なら `scroll_speed < 60`）が棒幅 1 列を割るので、棒は鳥列に必ず 1 フレーム重なる。**縦**: `vy_max` キャップで 1 フレームの落下が `vy_max × DT < 1 行` に収まる。両軸とも「1 フレーム 1 セル以内」なのでスイープ判定は不要。`new()` で `assert!(scroll_speed * DT < 1.0 && vy_max * DT < 1.0)` し、§7 の体感チューニングで不変条件が破れないよう担保する。

### 乱数
**決定論（同一 `(seed, 入力列, ステップ数)` → ビット一致。物理は固定 DT で進むので native/wasm・実機/headless が一致）が要件**のため、OS エントロピーを引く `getrandom`/`rand` は使わない（getrandom 0.4 は `wasm_js` feature だけで wasm 対応はするが、非決定なので要件に反する）。**自前 RNG（数行）** を `rng.rs` に置く。実装は **SplitMix64**（seed=0 でも縮退せず、`Date.now()` のような単調 seed でも初手から散る。素の xorshift は state=0 で 0 を吐き続けるため避ける）。seed は呼び出し側が渡す（term: システム時刻、web: `Date.now()`）。これで core は完全に依存ゼロ・全環境同一動作。

**決定論ガードレール**: core は `f32` の四則演算と比較のみを使い、`mul_add`・`sqrt`・三角/指数関数を**使わない**（IEEE754 の基本演算は native/wasm でビット一致するが、transcendental と FMA 融合は保証されない）。物理は**常に固定 `DT = 1.0/60.0`** で進む（§1 の蓄積ループ）ため、実機・headless・テストが同一トレースになる。実 dt の揺れは描画頻度にのみ影響し、物理・判定には一切入らない。入力は描画フレーム境界で反映され（1 描画内の複数 tick は同一入力状態を見る）、決定論の「入力列」はこの量子化済みの列を指す。

### テスト（= 検証可能ゴール）
`crates/core/src/lib.rs` の `#[cfg(test)]` に:
- `tick` を重ねると bird_y が増加（重力で落下）
- `flap` 後に bird_vy が負（上昇）
- 隙間の外に棒が来る位置で tick → `Phase::GameOver`
- 棒が鳥 x を通過すると score == 1
- 地面/天井到達で GameOver
- **同一 seed で pipe 生成列が完全一致**（決定論）
- スクリプト化したフラップ列で N tick 回し、想定スコアに到達（簡易シミュレーション）

---

## 4. term クレート（flappy-term）— crossterm でターミナル描画

依存は **crossterm のみ**（ratatui は使わない＝1セル1文字のドット表現にはオーバースペック）。

- 起動時: alternate screen 入場 + raw mode + カーソル非表示 + **mouse capture 有効化**（クリック入力を取るため。実行中は端末のテキスト選択・コピーが効かなくなるが、クリック操作を受ける以上必要な**意図的な許容差**）。crossterm 0.29 に raw mode の RAII は無いので**自前の `Drop` ガード**で復帰させ、加えて **panic hook**（まず端末復帰 → 既定 hook）を仕込んで panic 時の端末破壊を防ぐ。**`panic = "abort"` を設定しない**（Drop が走らなくなる）。
- ゲームループ: 描画は ~30–60Hz、**物理は固定 60Hz**（§1 の蓄積ループ。1 描画あたり 1–2 tick）。`event::poll(timeout)` で非ブロッキング入力 →
  - **Space / クリック**: GameOver なら `restart()`、それ以外は `flap()` / **r**: `restart()`（全 phase で即リスタート）/ **q・Esc**: 終了
- 各フレーム: 経過実時間を蓄積し固定 `DT` 刻みで `tick()` を呼ぶ → グリッド（`Vec<char>` か `String`）を組み立て、カーソルを左上に戻して一括描画。
  - 鳥と棒は Braille サブセル描画（1 セル = 横2×縦4 ドット。鳥は 2×2 ドットブロブ、死亡時のみ `✕`・赤。棒は緑）、地面ライン、上部にスコア/ベスト、Ready/GameOver のメッセージ。
- グリッドはターミナル幅未満ならセンタリング（レターボックス）。64×24 未満ならプレイを止めてリサイズを促す（§2）。`Event::Resize` は次フレームで再センタリングのみ。
- **headless モード** `flappy --headless --seed S --frames N`: TTY 不要・端末ガード非経由で、**決定論的な autopilot**（下記の一意規則で隙間を追従）＋**固定 DT** で N フレーム自動実行し最終スコアを stdout 出力。CI は **(a) 同一 seed の 2 回走でスコア一致**（決定論の回帰検出）と **(b) スコアが既知のゴールデン値（非ゼロ）** を assert する（ゴールデン値は実装後に実測して埋める）。autopilot 規則は実装非依存に一意化する:

  ```
  // 前方(x≥bird_col)の未passed の最寄り、無ければ未passed の最寄りを狙う（x は f32 なので partial_cmp）
  target = pipes.filter(|p| !p.passed && p.x >= bird_col).min_by(|a,b| a.x.partial_cmp(&b.x).unwrap())
           .or(pipes.filter(|p| !p.passed).min_by(|a,b| a.x.partial_cmp(&b.x).unwrap()))
  if let Some(p) = target {
      // bird_cell().1 = bird_y.round()（衝突と同じ丸め。bird_y は内部なので公開ゲッター経由）
      if (bird_cell().1 as f32) > p.gap_top as f32 + pipe_gap as f32 / 2.0 { flap() }
  }
  ```
- bin 名は `flappy`（`cargo run -p flappy-term` / インストール後 `flappy`）。

---

## 5. web クレート（flappy-web）— web-sys で canvas 描画

- 通常の **binary（`fn main()`）を wasm32 にビルド**（cdylib ではない）。依存: `wasm-bindgen`, `web-sys`（Window/Document/HtmlCanvasElement/CanvasRenderingContext2d/KeyboardEvent 等）, `flappy-core`。イベントリスナの定型ボイラープレート削減に `gloo-events` を使用（RAF は `web-sys` の `request_animation_frame` を直接利用する正準クロージャパターン。gloo-render は使わない＝依存最小）。
- `fn main()` をエントリに（trunk の no-modules 構成では `#[wasm_bindgen(start)]` 不要）: canvas 取得 → 入力リスナ登録（**Space/click/tap**。GameOver なら `restart()`、それ以外は `flap()`＝term と同一ルーティング。`keydown` のリピート `event.repeat` は無視。term はリピートを区別できないため挙動が異なる——§1 の許容差を参照）→ requestAnimationFrame ループ開始。Space の `preventDefault` は **passive でないリスナ**（gloo の `EventListenerOptions::enable_prevent_default()`）でないと無視されるので注意。
- RAF ループ: 前フレームからの実時間を蓄積し**固定 `DT` 刻みで `tick()`**（§1。1フレーム上限 0.10s）。RAF ハンドルは **drop で停止**するので構造体に保持するか `forget()` する（gloo-events のリスナ保持と同じ作法）。**`visibilitychange` で非表示中はループを止め、復帰時に `acc=0` にリセット（必須）**——長時間バックグラウンド後の復帰一発死を防ぐ（0.10s クランプは描画ヒッチ用の二次的な安全網）。描画は core の状態を canvas に矩形で。1セル=固定 px（例 16px）、canvas = `64*16 × 24*16`、CSS で中央寄せ。**高 DPI** は `image-rendering: pixelated` ＋整数座標描画で滲み回避（`devicePixelRatio` スケールは任意）。色は term と揃える（恐竜風の淡背景＋濃色要素、棒は緑）。
- 描画ロジックは term と同じ「core グリッドをなぞって塗る」構造（言語も手順も共通）。

---

## 6. ビルド & 配信

### 必要なツールチェーン（初回のみ）
```bash
rustup target add wasm32-unknown-unknown
cargo install trunk            # 現状未導入。wasm-pack より app 向きで dev server 付き
```
`rust-toolchain.toml` に wasm32 ターゲットを記載して再現性を確保。

### ローカル
- ターミナル: `cargo run -p flappy-term`
- ブラウザ: `cd crates/web && trunk serve` → http://localhost:8080

### 配信（静的ビルド）
- `cd crates/web && trunk build --release --public-url /flappy-cli/` → `crates/web/dist/`（GitHub Pages のサブパス対応）。
- `.github/workflows/pages.yml`: main への push で trunk build → `dist/` を GitHub Pages へ自動デプロイ（§10 CD）。

---

## 7. ゲームパラメータ初期値（Config デフォルト、要・体感チューニング）

| パラメータ | 初期値 | 備考 |
|---|---|---|
| cols × rows | 64 × 24 | 両環境共通の論理グリッド |
| bird_col | 12.0 | 鳥の固定 x |
| gravity | 45.0 行/s² | フワッと感。やや難しめなら gravity↓(30〜38)・滞空↑方向で調整 |
| flap_impulse | -16.0 行/s | 上向き初速 |
| scroll_speed | 12.0 列/s | 横スクロール速度 |
| pipe_gap | 6 行 | 隙間の縦幅 |
| pipe_spacing | 22.0 列 | 棒の間隔 |
| vy_max | 30.0 行/s | 終端速度（下向き上限。`vy_max × DT < 1` 行/フレーム） |

棒は固定で **1 列幅**（`pipe_width` は Config 化しない）。境界は HUD=行0・地面=行 `rows-1`（鳥が乗れる範囲 `1..=rows-2`）。隙間 `gap_top` は `[1, rows-1-pipe_gap]`（両端含む）から rng で選ぶ。初期化時は最初の棒を `x = cols` に置き `dist_to_next = pipe_spacing` から開始（1本目到達まで約1画面の助走、開始即死を防ぐ）。**不変条件 `scroll_speed × DT < 1.0` かつ `vy_max × DT < 1.0`**（DT=1/60 → scroll<60。離散セル判定の取りこぼし防止）を `new()` で assert する。

数値は「人が数本くぐれる」体感で最終調整する（成功条件）。難易度漸増（速度up等）は v1 では入れず、調整しやすいよう Config に寄せておく（後付け容易）。最高スコアはセッション内メモリ保持のみ（ファイル永続化は v1 では入れない）。

> **将来メモ（衝突判定の切り替え）**: 衝突は v1 では離散の等値判定（`pipe_col == bird_c`）で、不変条件 `scroll_speed × DT < 1.0`（DT=1/60 → `scroll_speed < 60`）の範囲でのみ正しい。難易度漸増などで `scroll_speed` をこの上限に近づける／超える場合は、**等値判定を区間判定（CCD: 移動前 x と移動後 x で鳥列 `[bird_col-0.5, bird_col+0.5)` を跨いだかで判定）に切り替える**こと。切り替えれば速度上限は消える。縦も同様に `vy_max × DT < 1.0` を維持する（超えるなら縦も掃く）。現状は `new()` の assert がこの閾値超えを検出するので、越えた時点で着手すればよい。

---

## 8. 検証（end-to-end）

1. **core**: `cargo test -p flappy-core` が全通過（重力・フラップ・衝突・スコア・決定論・簡易シミュレーション）。
2. **term**: `cargo run -p flappy-term` で実際に数本くぐれることを目視。加えて `flappy --headless --seed 1 --frames 600`（隙間追従 autopilot・固定 DT）が **2 回走で一致する非ゼロのゴールデンスコア**を出す（TTY不要のスモーク。詳細は §10）。
3. **web**: `cd crates/web && trunk serve` 起動 → **Playwright MCP** で http://localhost:8080 を開き、Space 押下 → スクリーンショットで描画と反応を確認。`trunk build --release` が `dist/` を生成。
4. **配信**: `trunk build --release --public-url /flappy-cli/` 成功。Pages ワークフロー追加時は Actions が緑になることを確認。

---

## 9. 実装順（各ステップに検証を紐付け）

1. workspace + `rust-toolchain.toml` + core クレート（型・物理・rng・テスト）
   → **検証**: `cargo test -p flappy-core` 通過
2. term レンダラ（crossterm 描画 + 入力 + RAII 復帰 + headless）
   → **検証**: 目視プレイ OK ＋ headless スコア再現
3. web レンダラ（web-sys canvas + 入力 + RAF）＋ `trunk serve`
   → **検証**: Playwright MCP で起動・Space 反応・スクショ確認
4. 配信設定（`trunk build --release` ＋ Pages ワークフロー）
   → **検証**: `dist/` 生成、Actions 緑

### 触る主なファイル
- `Cargo.toml`(workspace), `rust-toolchain.toml`
- `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/rng.rs`
- `crates/term/Cargo.toml`, `crates/term/src/main.rs`
- `crates/web/Cargo.toml`, `crates/web/src/main.rs`, `crates/web/index.html`
- `.github/workflows/ci.yml`（品質ゲート）, `.github/workflows/pages.yml`（配信）

---

## 10. テスト & CI 設計

### テスト層

| 層 | 対象 | 手段 | 自動化 |
|---|---|---|---|
| core 単体 | 物理・衝突・スコア・決定論 | `#[cfg(test)]`（§3 のリスト） | CI |
| レンダリング（term のみ） | core 状態 → 文字グリッドの純粋関数（`scene::render`） | ゴールデン（既知状態の1フレームを文字列比較） | CI |
| 統合（term） | 引数解析〜tick ループ〜終了 | headless で seed 固定 → 期待スコア（ゴールデン値）を assert | CI |
| web | wasm へのコンパイル可否 | `trunk build`（ロジックは core で検証済み、web 固有の重テストは不要） | CI |
| 手動 | 体感・見た目 | term 目視 / web は Playwright MCP でスクショ | 手動 |

- **レンダリングのゴールデンテスト（term のみ）**: 衝突判定とセル丸めが描画と一致していること（「隙間を通ったのに死ぬ」系バグ）をコードで保証するため、`core 状態 → 文字グリッド` の純粋関数（`scene::render`）をテストする。web の描画は canvas 直叩きで純粋層を持たないため、ゴールデンは適用しない（canvas の純粋層化は過剰）。term/web の描画一致は、占有述語（`pipe_blocks_row` / `bird_cell`）と文言定数を core から共有することに加え、Playwright での手動確認で担保する。
- **headless 統合テスト**: autopilot（隙間追従の決定論的フラップ規則）＋固定 DT で `--seed 1 --frames 600` を回し、**(a) 同一 seed の 2 回走で一致**＋**(b) 固定の非ゼロゴールデンスコア**を assert。core 単体 #7（特定フラップ列→スコア）がロジック単体、headless がバイナリ全体の統合スモーク、と役割分担する。
- （任意・nice-to-have）**native↔wasm 決定論の厳密証明**: core の決定論テストを `wasm-bindgen-test` で wasm32 上でも回す（core に dev-dependency のみ追加）。native テスト＋「transcendental 不使用」規約で実用上は十分なので必須ではない。

### CI（`.github/workflows/ci.yml` — main push / PR / 手動で実行）

品質ゲート CI は 2 ジョブ構成で稼働中:

- **ci ジョブ**（workspace root）
  1. `cargo fmt --all --check`
  2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
  3. `cargo test --workspace --locked`（core 単体＋レンダリングゴールデン＋headless 統合）
- **wasm ジョブ**（`crates/web`。web は root workspace から exclude のため専用ジョブ）
  1. `cargo fmt --all --check`
  2. `cargo clippy --target wasm32-unknown-unknown --all-targets --locked -- -D warnings`
  3. `trunk build --locked`（wasm コンパイル確認。trunk はバージョン固定）

Rust の標準ゲートで過剰ではない。OS matrix 等は v1 では不要。

### セキュリティ監査（`.github/workflows/audit.yml`）

- `cargo audit` で依存クレートの既知脆弱性（RUSTSEC）を検査。root と `crates/web` の両 lock が対象。
- トリガ: lock / manifest / audit.yml 変更を含む push・PR、毎週月曜（schedule）、手動（workflow_dispatch）。

### CD（`.github/workflows/pages.yml` — main push で自動実行）

- `trunk build --release --public-url /flappy-cli/` → `dist/` を GitHub Pages へデプロイ。
- `dist/` に `.nojekyll` を置く（`_` 始まりファイル対策）。

### リリース（`.github/workflows/release.yml` — `v*` タグ push で実行）

- tag と各 Cargo.toml の version 一致を検証後、`flappy` バイナリを 3 ターゲット（x86_64 / aarch64 の linux-musl、aarch64-apple-darwin）でクロスビルドし、tarball + SHA256SUMS を GitHub Releases に添付する（手順は `docs/RELEASE.md`）。
