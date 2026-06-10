# flappy-cli

[![CI](https://github.com/astail/flappy-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/astail/flappy-cli/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

ターミナル（mac / ubuntu）とブラウザの両方で動く Flappy Bird 系のドットゲーム。スペースで上昇・重力で落下し、横スクロールする上下の棒の隙間を抜け続けるエンドレス（スコア制）。見た目は Chrome 恐竜ゲーム風のシンプルなドット絵。

🎮 **ブラウザ版で今すぐ遊ぶ**: <https://astail.github.io/flappy-cli/>

## 特徴

- **1 ロジック・2 レンダラ**: ゲームロジックを I/O を持たない純粋な `flappy-core` に集約し、ターミナル / ブラウザは薄いレンダラに徹する。Rust 1 言語でロジックを完全共有。
- **決定論的 state machine**: `tick(dt)` 駆動で Ready / Playing / GameOver を遷移。同じシードなら同じ棒の並びを再現。
- **エンドレス・スコア制**: 棒を 1 本抜けるごとに +1 点。最高スコアを競う。

## アーキテクチャ / スタック

**Rust + WASM**。クレート構成は以下のとおり。

| クレート | 名前 | 役割 |
|---|---|---|
| `crates/core` | flappy-core | 純粋なゲームロジック（依存ゼロ、SplitMix64 RNG 同梱） |
| `crates/term` | flappy-term（bin 名 `flappy`） | crossterm でターミナル描画 |
| `crates/web` | flappy-web | web-sys で canvas 描画（wasm、trunk でビルド） |

> `crates/web` は wasm 専用でホストビルド不可のため、native workspace（`crates/core` / `crates/term`）からは除外され、独立した workspace として扱われる。

## 操作

| キー / 操作 | 動作 |
|---|---|
| Space / 左クリック / タップ | 上昇（ゲーム中）/ 開始 |
| `r` / `R` | リスタート（GameOver 時） |
| `q` / `Q` / Esc | 終了（ターミナル版のみ） |

## インストール

### バイナリリリース（ターミナル版）

[Releases](https://github.com/astail/flappy-cli/releases) から各プラットフォーム向けの `flappy-<version>-<target>.tar.gz` を取得して展開する。

| プラットフォーム | target |
|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` |
| Linux aarch64 | `aarch64-unknown-linux-musl` |
| macOS (Apple Silicon) | `aarch64-apple-darwin` |

```bash
# 例: Linux x86_64
VERSION=0.2.0
curl -LO https://github.com/astail/flappy-cli/releases/download/v${VERSION}/flappy-${VERSION}-x86_64-unknown-linux-musl.tar.gz
# SHA256SUMS で整合性を検証（任意）
curl -LO https://github.com/astail/flappy-cli/releases/download/v${VERSION}/SHA256SUMS
shasum -a 256 -c SHA256SUMS --ignore-missing
tar xzf flappy-${VERSION}-x86_64-unknown-linux-musl.tar.gz
./flappy-${VERSION}-x86_64-unknown-linux-musl/flappy
```

### ブラウザ版

インストール不要。<https://astail.github.io/flappy-cli/> をブラウザで開く。

## 動かし方（ソースからビルド）

事前に Rust ツールチェインが必要（`rust-toolchain.toml` で channel `1.95.0` と `wasm32-unknown-unknown` ターゲットを固定済み）。

### ターミナルで遊ぶ

```bash
cargo run -p flappy-term
```

### テストを実行する

```bash
cargo test -p flappy-core
```

### ブラウザで開発する

初回のみ wasm ターゲットと trunk を用意する。

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
```

開発サーバを起動する（ホットリロード）。

```bash
cd crates/web && trunk serve
```

### ブラウザ配信ビルド

GitHub Pages のサブパス（`/flappy-cli/`）向けにビルドする。

```bash
cd crates/web && trunk build --release --public-url /flappy-cli/
```

## デプロイ

ブラウザ版は `main` への push をトリガに GitHub Actions（`.github/workflows/pages.yml`）が `trunk build` し、GitHub Pages へ自動デプロイされる。公開先は <https://astail.github.io/flappy-cli/>。

## 設計ドキュメント

アーキテクチャ / フロー / 画面レイアウト / core API / パラメータ / 検証の詳細は [`docs/DESIGN.md`](docs/DESIGN.md) を参照。
