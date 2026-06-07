# Flappy CLI — 設計ドキュメント

ターミナル（mac/ubuntu）とブラウザの両方で動く Flappy Bird 系のドットゲーム。
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
│   │   └── src/lib.rs      #   + rng.rs (xorshift)
│   ├── term/               # flappy-term: crossterm でターミナル描画（bin名 `flappy`）
│   │   └── src/main.rs
│   └── web/                # flappy-web: web-sys で canvas 描画（wasm bin）
│       ├── index.html      #   trunk のエントリ
│       └── src/main.rs
└── .github/workflows/pages.yml   # 任意: GitHub Pages 自動配信
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
                ▲                                                    │
                └──────────────── SPACE / r （restart）──────────────┘

   q / Esc（term）・ウィンドウを閉じる（web） … いつでも終了
```

### 入力 → 効果

| 状態 | キー / 操作 | 効果 |
|---|---|---|
| Ready | SPACE / クリック・タップ | ゲーム開始（→Playing、初回フラップ込み） |
| Playing | SPACE / クリック・タップ | フラップ（上昇） |
| Playing | （放置） | 重力で落下し続ける |
| GameOver | SPACE / r / クリック | リスタート（→Ready） |
| 全状態 | q / Esc（term のみ） | 終了 |

web の Space は `preventDefault` でページスクロールを抑止する。

### ゲームループ（term / web 共通の流れ）

`tick(dt)` 駆動なので、両プラットフォームは「実 dt を測って core を進め、状態を描画する」だけ。

```
last = now()
loop {
    t = now();  dt = min(t - last, 0.05);  last = t   // dt は 50ms でクランプ
    for ev in poll_input() {            // 非ブロッキング入力
        Space/Click => game.flap()       //   Ready 中は開始も兼ねる
        R           => game.restart()    //   GameOver 中のみ
        Q/Esc       => quit              //   term のみ
    }
    game.tick(dt)        // Playing 時だけ物理更新（Ready/GameOver は静止）
    render(&game)        // core の状態 → 文字グリッド / canvas 矩形
    // term: 約 33ms スリープ（~30FPS） / web: requestAnimationFrame で次フレーム
}
```

---

## 2. 画面レイアウト

論理グリッドは **64列 × 24行**（両プラットフォーム共通＝同じ画面）。下記は縮小イメージ。
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
                                          
         ╔════════════════════╗           
         ║     GAME  OVER      ║           
         ║     SCORE    7      ║           
         ║  SPACE / r : retry  ║           
    ✕    ║  q         : quit   ║           
         ╚════════════════════╝           
──────────────────────────────────────────
```

### 要素の対応（term ⇄ web で見た目を揃える）

| 要素 | term（文字） | web（canvas） |
|---|---|---|
| 鳥 | `●` | 塗り円 / 角丸矩形 |
| 棒 | `█`（緑） | 緑の矩形 |
| 地面ライン | `─` の横帯（最下行） | 同位置の横帯 |
| HUD（SCORE/BEST） | 最上行テキスト | 上部テキスト |
| メッセージ枠 | 罫線ボックス | canvas テキスト（必要なら枠） |
| 背景 | 端末既定（暗） | 淡色（恐竜ゲーム風） |

- グリッドより端末が広い場合はセンタリング（レターボックス）。狭すぎる場合は警告を出す。
- web の canvas は 1セル=固定 px（例 16px）→ `64*16 × 24*16` を CSS で中央寄せ。

---

## 3. core クレート（flappy-core）— 依存ゼロ・純粋ロジック

**最重要。両プラットフォームが共有する唯一の真実。** I/O・描画・sleep・乱数エントロピー取得を一切持たない。`tick(dt)` 駆動の決定論的な状態機械。

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
- `tick(&mut self, dt: f32)` — Playing 時のみ物理更新（後述）。dt は呼び出し側で **≤0.05 にクランプ**して渡す
- `restart(&mut self)` — best を保持して初期化
- 描画用ゲッター: `phase()`, `bird_cell() -> (u16,u16)`, `pipes()`, `score`, `best`

### tick の中身
1. `bird_vy += gravity * dt; bird_y += bird_vy * dt`
2. 全 pipe の `x -= scroll_speed * dt`、画面外(x < -1)の pipe を除去
3. `dist_to_next -= scroll_speed * dt`、0 以下になったら隙間位置を rng で決めて新 pipe を右端に生成、`dist_to_next += pipe_spacing`
4. 鳥 x を通過した未 passed pipe があれば `score += 1; passed = true`（best も更新）
5. **衝突判定** → 当たれば `phase = GameOver`
   - 天井/地面: `bird_y < 0 || bird_y >= rows - GROUND`
   - 棒: 鳥セルが bird_col 上の pipe と重なり、かつ隙間 `[gap_top, gap_top+pipe_gap)` の外

### 乱数
`rand` / `getrandom` は wasm で追加設定が要るため使わない。**自前の xorshift（数行）** を `rng.rs` に置く。seed は呼び出し側が渡す（term: システム時刻、web: `Date.now()`）。これで core は完全に依存ゼロ・全環境同一動作。

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

- 起動時: alternate screen 入場 + raw mode + カーソル非表示。**RAII ガードで panic/終了時に必ず復帰**。
- ゲームループ: 約 30FPS（33ms）。`event::poll(timeout)` で非ブロッキング入力 →
  - **Space**: `flap()` / **r**: `restart()`（GameOver時）/ **q・Esc**: 終了
- 各フレーム: 経過 dt を計測 → `tick(dt)` → グリッド（`Vec<char>` か `String`）を組み立て、カーソルを左上に戻して一括描画。
  - 鳥 `●`、棒 `█`（緑）、地面ライン、上部にスコア/ベスト、Ready/GameOver のメッセージ。
- グリッドはターミナル幅未満ならセンタリング（レターボックス）。狭すぎる場合は警告。
- **headless モード** `flappy --headless --seed S --frames N`: TTY 不要で N フレーム自動実行し最終スコアを stdout 出力 → CI/非対話での決定論スモークテストに使う。
- bin 名は `flappy`（`cargo run -p flappy-term` / インストール後 `flappy`）。

---

## 5. web クレート（flappy-web）— web-sys で canvas 描画

- crate-type は wasm bin。依存: `wasm-bindgen`, `web-sys`（Window/Document/HtmlCanvasElement/CanvasRenderingContext2d/KeyboardEvent 等）, `flappy-core`。RAF/イベントの定型ボイラープレート削減に `gloo`（gloo-render, gloo-events）を併用。
- `main()`（`fn main()`）で: canvas 取得 → 入力リスナ登録（**Space/click/tap で flap**、Space は `preventDefault` でページスクロール抑止、GameOver中は再開）→ requestAnimationFrame ループ開始。
- RAF ループ: 前フレームからの実 dt（≤0.05 クランプ）を `tick` に渡し、core の状態を canvas に矩形描画。1セル=固定 px（例 16px）、canvas = `64*16 × 24*16`、CSS で中央寄せ。色は term と揃える（恐竜風の淡背景＋濃色要素、棒は緑）。
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
- `.github/workflows/pages.yml`（任意）: push 時に trunk build → `dist/` を Pages へ自動デプロイ。

---

## 7. ゲームパラメータ初期値（Config デフォルト、要・体感チューニング）

| パラメータ | 初期値 | 備考 |
|---|---|---|
| cols × rows | 64 × 24 | 両環境共通の論理グリッド |
| bird_col | 12.0 | 鳥の固定 x |
| gravity | 45.0 行/s² | フワッと感。要調整 |
| flap_impulse | -16.0 行/s | 上向き初速 |
| scroll_speed | 12.0 列/s | 横スクロール速度 |
| pipe_gap | 6 行 | 隙間の縦幅 |
| pipe_spacing | 22.0 列 | 棒の間隔 |

数値は「人が数本くぐれる」体感で最終調整する（成功条件）。難易度漸増（速度up等）は v1 では入れず、調整しやすいよう Config に寄せておく（後付け容易）。最高スコアはセッション内メモリ保持のみ（ファイル永続化は v1 では入れない）。

---

## 8. 検証（end-to-end）

1. **core**: `cargo test -p flappy-core` が全通過（重力・フラップ・衝突・スコア・決定論・簡易シミュレーション）。
2. **term**: `cargo run -p flappy-term` で実際に数本くぐれることを目視。加えて `flappy --headless --seed 1 --frames 600` が決定論的に同じスコアを出す（TTY不要のスモーク）。
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
4. 配信設定（`trunk build --release` ＋ 任意で Pages ワークフロー）
   → **検証**: `dist/` 生成、（任意）Actions 緑

### 触る主なファイル
- `Cargo.toml`(workspace), `rust-toolchain.toml`
- `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/rng.rs`
- `crates/term/Cargo.toml`, `crates/term/src/main.rs`
- `crates/web/Cargo.toml`, `crates/web/src/main.rs`, `crates/web/index.html`
- `.github/workflows/pages.yml`（任意）
