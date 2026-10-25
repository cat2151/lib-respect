# lib-respect

自分のリポジトリの依存ライブラリを可視化し、その作者のGitHubをリスペクトを込めて訪問できるTUI。Rustで書かれています。

## これまでの課題と、このアプリが解決すること
- Rustのエコシステムは素晴らしいですね！いつも誰かのライブラリのお世話になっています。リスペクトを込めてスターをつけたい！
- あれ？このライブラリ、スターしたっけ…？あっ漏れてた。スターが漏れてるライブラリを可視化して、リスペクトを込めて作者のGitHubに訪問したい！
- そういうわけで作りました。主にRustがメインですが、CMakeの一部にも対応しました。

## インストール

Rustが必要です。

```sh
cargo install --force --git https://github.com/cat2151/lib-respect
```

## 実行

```sh
lib-respect
```

## 更新の確認

```sh
lib-respect check
```

ビルド時のコミットとGitHubの `main` ブランチの最新コミットを比較します。`embedded` にビルド時、`remote` に最新のコミットハッシュを表示し、`result` に `up-to-date`（一致）または `update available`（差分あり）を表示します。確認にはgitが必要です。

## アップデート

```sh
lib-respect update
```

別プロセスで `cargo install --force --git https://github.com/cat2151/lib-respect` を実行し、本体は終了します。更新完了後は手動で `lib-respect` を再起動してください。更新にはRust・git・PATH上のPythonが必要です。更新コマンド自体が動作しない場合は、インストールコマンドを再実行してください。
