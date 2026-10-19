# FAQ

## TETR.IO 側にバグ報告してもいいですか？

**いいえ。** このライブラリの問題を TETR.IO の関係者へ報告しないでください。
作者とは一切関係のない非公式のものです。

API 自体の不具合（と思われるもの）を見つけた場合は、
[本家の issue tracker](https://github.com/tetrio/issues) のルールに従ってください。

## なぜ「API の落とし穴」を直さずに吸収しているのですか？

直す権利がないからです。TETRA CHANNEL API は予告なく変わる前提で運用されています。
「変わることを前提に、変わっても壊れにくいクライアント」を作るのがこのライブラリの立場です。

また、フォーク元の C# 版を書いたときの知見として、
**上流が変わるたびにクライアントを直す**運用は現実的ではありませんでした。

## `pb` はなぜ使えないのですか？

`/users/:user/records/:gamemode/:leaderboard` の `:leaderboard` に
入れてよいのは `top` / `recent` / `progression` だけだからです。
`pb` を渡すと `422` が返ります。このライブラリは `pb` を `progression` に直します。

## `40l` を渡したのに空でした

API は `/records/:leaderboard` に合成 ID（`40l_global` など）を要求します。
単に `40l` を渡すと `200` と `entries: []` が返り、**エラーになりません**。
このライブラリは `40l` を `40l_global` に展開します。

## 存在しないユーザーなのか、通信エラーなのか分かりません

```rust
Err(err) if err.is_likely_user_mistake() => { /* 名前の誤り */ }
Err(err) if err.is_retryable()          => { /* 一時的な障害 */ }
```

## `games_played` が `null` なのはなぜですか？

そのプレイヤーが統計を非公開にしているからです。API は `-1` を返します。
「0 試合」ではないので `0` にはしません。

## 未ランクなのに `rank` が `"z"` と出ます

このライブラリ経由では `null` になります。`"z"` を直接扱っている場合は、
`jval::as_rank` を通してください。

## 非同期版はありますか？

ありません。理由は [アーキテクチャ](Architecture) に書いています。
TETRA CHANNEL は素の JSON-over-HTTPS なので、同期の方が
スロットルとバックオフを正確に書けます。

並行取得が必要なら、`Fetcher` をスレッドごとに持ってください
（内部に `RefCell` のキャッシュを持つため `Sync` ではありません）。
スレッドを分けるとスロットルも分かれる点に注意してください。

## ブラウザ（WASM）で使えますか？

そのままでは使えません。

- `access-control-allow-origin` が `https://tetr.io` に限定されています
- `X-Session-ID` が CORS の許可ヘッダーに含まれていません

ブラウザから使うなら、サーバー側でプロキシしてください。

## レート制限に引っかかりました

`min_interval`（既定 1 秒）を下げていないか確認してください。
また `cache.cached_until` を尊重していれば、同じデータの再取得は発生しません。
`TransportConfig::session_id` を設定すると、ページングの一貫性も上がります。

## 依存が少ないのはなぜですか？

`serde` / `serde_json` / `ureq` / `thiserror` だけです。
`ureq` は `rustls` + `webpki-roots` を使うので、**システムの CA ストアに依存しません**。
proot・Android・コンテナなど `/etc/ssl` が怪しい環境でも動きます
（このライブラリ自体、Android 上の proot で開発・検証されました）。

## C# 版との関係は？

作者の別リポジトリ（`TetrioFetcher-CSharp` / `TetrioFetcher_CSharpV2`）の
Rust 版という位置づけです。API 層を書き直し、次の点を直しています。

- C# 版の `DelayAccess` は `WaitThreadCount * 1000` で待つため、
  同時実行時に待ち時間の計算がずれる
- `Cache` の `_ => throw new NotImplementedException()` は、
  新しい `cache.status` が来た瞬間に落ちる
- `JsonNode` の `ToString()` 経由の数値パースは、型が揺れると壊れる

## ライセンスは？

MIT です。

## コントリビュートしたい

Issue / PR は歓迎です。特に**新しい API の落とし穴**の報告が助かります。
その場合は `curl` の再現手順と応答を添えてください。
`docs/api-quirks.md` に追記していきます。
