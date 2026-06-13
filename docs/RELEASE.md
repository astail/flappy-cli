# リリース手順

`flappy` バイナリの新バージョンを GitHub Releases として公開し、Homebrew tap を更新するまでの手順。
v0.2.0 のリリースで実際に踏んだ流れをそのまま記す。例として `0.1.0` → `0.2.0` を上げる場合を書く。

> 前提: リリースは **`vX.Y.Z` タグの push** で起動する（`.github/workflows/release.yml`）。
> タグは `crates/term/Cargo.toml` の `version` と一致していなければ CI で fail する（tag = version の担保）。

## 全体像

```
1. バージョン bump（PR）→ 検証: cargo test --workspace --locked が通る
2. PR をマージ        → 検証: CI（ci / wasm / audit）が全グリーン
3. vX.Y.Z タグを push → 検証: release.yml が成功し GitHub Release が公開される
4. Homebrew tap 更新  → 検証: brew install / brew test が通る
```

---

## 1. バージョン bump（PR）

バージョンは複数箇所に散っている。**すべて同じ値に揃える**こと。

| 対象 | 役割 |
|---|---|
| `Cargo.toml`（root `[workspace.package]`） | core / term が `version.workspace = true` で継承する単一ソース。`release.yml` が **タグと照合**する |
| `crates/web/Cargo.toml` | web 版（独立 workspace のため継承不可、個別保持）。`release.yml` が **タグと照合**する |
| `README.md` のインストール例 | `VERSION=X.Y.Z` |

（`crates/core/Cargo.toml` / `crates/term/Cargo.toml` は root から継承するため書き換え不要。`crates/term/src/golden/ready.txt` は version をトークン `vX.Y.Z` で保持するため更新不要）

```bash
git checkout main && git pull
git checkout -b release/v0.2.0

# root Cargo.toml [workspace.package] と crates/web/Cargo.toml の version を書き換える（0.1.0 → 0.2.0）
# （core / term は version.workspace = true で root を継承するため書き換え不要）

# Cargo.lock を同期（workspace member は Cargo.toml の値に追従するので -w で十分。
# web は root から exclude された独立 workspace なので別途）
cargo update -w
( cd crates/web && cargo update -w )

# README の version 文字列を 0.2.0 に更新
#   README.md の `VERSION=0.1.0` → `VERSION=0.2.0`
```

検証（push 前にローカルで CI と同じチェックを通す）:

```bash
cargo test --workspace --locked   # golden test を含め全パスすること（Cargo.lock が未同期なら --locked が fail して検出できる）
```

PR を作成し、CI（ci / wasm / audit）がグリーンになったら squash merge する。

## 2. タグを push してリリース発行

```bash
git checkout main && git pull          # マージ済みコミットを取り込む

# term/Cargo.toml の version とタグが一致していることを確認
grep -m1 '^version' crates/term/Cargo.toml   # → version = "0.2.0"

git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

`release.yml` が起動し、以下を実行する:

1. **verify-version** — タグ（`v0.2.0` → `0.2.0`）と `crates/term/Cargo.toml` の version を照合
2. **build** — 3 ターゲットをクロスコンパイルし `flappy-<version>-<target>.tar.gz`（バイナリ + README + LICENSE 同梱）を作成
   - `x86_64-unknown-linux-musl`
   - `aarch64-unknown-linux-musl`
   - `aarch64-apple-darwin`（Apple Silicon。**Intel mac 向けビルドは無い**）
3. **release** — `SHA256SUMS` を集約し GitHub Release を発行（リリースノート自動生成）

検証:

```bash
gh run watch                                   # release.yml の進行を確認
gh release view v0.2.0 --json url,assets       # tarball 3 種 + SHA256SUMS が添付されていること
```

## 3. Homebrew tap を更新

tap は別リポジトリ [`astail/homebrew-tap`](https://github.com/astail/homebrew-tap) の `Formula/flappy-cli.rb`。
**version と 3 つの `url` / `sha256` を手動で更新する**（自動 bump は未導入）。

```bash
# 公開済み Release の SHA256SUMS から各 tarball のハッシュを取得
curl -sL https://github.com/astail/flappy-cli/releases/download/v0.2.0/SHA256SUMS
```

`Formula/flappy-cli.rb` を編集:

- `version "0.2.0"`
- macOS arm / Linux x86_64 / Linux arm それぞれの `url` のバージョン文字列
- それぞれの `sha256`（上の `SHA256SUMS` の値）

formula の要点（flappy 固有）:

- **formula 名 = `flappy-cli`、コマンド名 = `flappy-cli`**。tarball 内の実体バイナリは `flappy` のため、`bin.install "flappy" => "flappy-cli"` でリネームしている。
- tarball は単一ルートディレクトリ構成で、Homebrew が 1 階層自動 strip するため `bin.install "flappy"` が解決できる。
- `test do` は TUI が TTY を要するため、TTY 不要の `flappy-cli --headless --frames 1`（スコア=数値を出力して終了）で動作検証する。

push 後の検証:

```bash
brew update
brew install astail/tap/flappy-cli
brew test astail/tap/flappy-cli       # headless 実行が数値を返すこと
flappy-cli --headless --frames 1      # → 数値
```

---

## チェックリスト

- [ ] 3 クレートの `Cargo.toml` version を更新
- [ ] `Cargo.lock` × 2 を同期
- [ ] `README.md` のインストール例 `VERSION` を更新
- [ ] `cargo test --workspace --locked` がローカルで通る
- [ ] PR の CI（ci / wasm / audit）が全グリーン → squash merge
- [ ] `vX.Y.Z` タグを push → `release.yml` 成功
- [ ] GitHub Release に tarball 3 種 + `SHA256SUMS` が添付
- [ ] `astail/homebrew-tap` の `Formula/flappy-cli.rb`（version / url / sha256）を更新
- [ ] `brew install` / `brew test` が通る

## dependabot 対象外の手動追従

dependabot は `taiki-e/install-action` の action 本体（SHA/version）は追従するが、`tool: trunk@X.Y.Z` で固定する trunk 本体（`.github/workflows/ci.yml` / `pages.yml` の 2 箇所）と `rust-toolchain.toml` の `channel` は対象外。リリース時（または定期的に）[trunk releases](https://github.com/trunk-rs/trunk/releases) と [Rust リリース](https://forge.rust-lang.org/) で最新 stable を手動確認して更新する。

## スコープ外（別 issue）

`.deb` / `.rpm` / crates.io 公開、Homebrew formula の自動 bump。
