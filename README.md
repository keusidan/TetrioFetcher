# TetrioFetcher

**TETR.IO の TETRA CHANNEL API の「壊れやすいところ」を吸収して、きれいな JSON を返す Rust クライアントです。**

> TETR.IO の関係者の方々とは一切関係ありません。このライブラリの問題を TETR.IO 側へ報告しないでください。
> 利用は [TETRA CHANNEL API](https://tetr.io/about/api) のみを対象とし、ゲーム本体の API は使いません。

## なぜ作ったか

TETRA CHANNEL API は今も活発に開発されていて、[公式ドキュメント](https://tetr.io/about/api)にも
「互換性はできるだけ保つが、予告なく変わる可能性がある」と書かれています。
実際に叩いて確認した結果、**同じ「失敗」が 5 通り以上の形で返ってきます**。

素直に書いたクライアントは、そのすべてを自分で処理することになります。
このクレートは、その処理を引き受けます。

| API の挙動 | 素直なクライアントが受け取るもの | このクレート |
|---|---|---|
| `users/{user}/records/40l/pb` — `pb` は存在しない leaderboard 名 | `422 … field 'leaderboard' is illegal` | `pb`/`pbs`/`personal` → `progression` に変換 |
| `/records/40l` — gamemode だけ渡す | `200` + `entries: []`（エラーにならない） | `40l_global` に展開 |
| `/users/KEUSIDAN` — 大文字 | `404`（しかも **HTML**） | ユーザー名を小文字化 |
| `/users/x/records/foo/top` — 打ち間違い | `200` + `entries: []`（「記録なし」と区別不能） | 送信前に `Invalid` エラー |
| `limit=0` / `999` / `abc` | 3 種類の異なる `422` | `1..=100` に丸める |
| `200 OK` + `{"success": false}` | HTTP 的には成功、パースで破綻 | `Error::Api`（`key`/`context` 付き） |
| `text/html` の 404 ページ | `serde_json` の `expected value at line 1 column 1` | `Error::NonJson`（中身の先頭を添えて） |
| `xp: 22336` と `xp: 68966508.611` | デシリアライズ時の型不一致 | `42` も `42.0` も `"42"` も受ける |
| `gamesplayed: -1`（非公開） | `-1` が実数として通る／`0` に丸めて「未プレイ」と誤認 | 非公開は `null`、`0` は `0` のまま |
| `tr: -1`, `rank: "z"`（未ランク） | `"z"` が階級として通る | `null` |
| `startTime: 9007199254740991` | 西暦 285428751 年の日付 | `null` |
| `aggregatestats.vsscore` と `vs` | 同じ意味なのに 2 つの名前 | 常に `vs` |
| `cache.cached_until`（ミリ秒） | 無視して再リクエスト | 尊重。同じ URL の再取得は通信ゼロ |
| `429` / `5xx` / 切断 | 1 回で失敗 | 指数バックオフで 3 回まで |
| 連打 | レート制限・ブロック | 全体で 1 req/秒にスロットル |

これらは推測ではなく、実際に API を叩いて確認した挙動です。
再現用の `curl` は [`docs/api-quirks.md`](docs/api-quirks.md) にまとめてあります。

## 使い方

```toml
[dependencies]
tetrio-fetcher = { git = "https://github.com/keusidan/TetrioFetcher" }
```

```rust
use tetrio_fetcher::Fetcher;

fn main() -> tetrio_fetcher::Result<()> {
    let fetcher = Fetcher::new();

    // 大文字でも OK。API は小文字しか受け付けません。
    let user = fetcher.user("Keusidan")?;
    println!("{} ({} XP)", user.username, user.xp);

    // 非公開の項目は None。0 や -1 ではありません。
    match user.games_played {
        Some(games) => println!("{games} 試合"),
        None => println!("対戦数は非公開"),
    }

    // `pb` は実在しない leaderboard 名ですが、変換されます。
    let pbs = fetcher.user_records("keusidan", "40l", "pb", 10)?;
    for pb in &pbs.entries {
        println!("{:?} 点 / {} 行", pb.stats.score, pb.stats.lines.unwrap_or(0));
    }

    // `40l` だけでも通ります（内部で 40l_global に展開）。
    let top = fetcher.records("40l", 5)?;

    Ok(())
}
```

同期 API です。TETRA CHANNEL は素の JSON を返すだけなので、非同期ランタイムを要求しません。
そのぶんスロットルとバックオフが素直に書けます（この API は 1 req/秒程度を求めています）。

### 主なメソッド

| メソッド | エンドポイント |
|---|---|
| `server_stats()` / `activity()` | `/general/stats`, `/general/activity` |
| `user()` / `user_league()` / `user_summaries()` | `/users/:user`, `/users/:user/summaries/league` |
| `user_personal_best()` | `/users/:user/summaries` から PB を取り出す |
| `user_records()` | `/users/:user/records/:gamemode/:leaderboard` |
| `user_leaderboard()` | `/users/by/:leaderboard` |
| `records()` | `/records/:leaderboard` |
| `score_flow()` / `league_flow()` / `league_ranks()` | `/labs/*` |
| `search_users()` | `/users/search/:query`（コロン不足を補う） |
| `raw_get()` | 未知のエンドポイント用 |

## CLI

```text
cargo install --path .

tfetch stats
tfetch user Keusidan
tfetch user-records keusidan 40l pb --limit 3
tfetch leaderboard league --country JP --limit 5
tfetch records 40l --limit 3
tfetch ranks
```

エラーは 1 行で stderr に出て終了コード 1 を返します。

```console
$ tfetch user nobody
error: TETRA CHANNEL API error (404): No such user! | Either you mistyped something, or the account no longer exists.
```

## 設計上の判断

- **パニックしない。** API 由来のデータで `unwrap` はしません。未知のフィールドは `None` に落ちるだけで、
  レスポンス全体のパースを巻き添えにしません。
- **黙って捨てない。** 正規化していない部分は `Record::raw_results` と `raw_get()` から取れます。
  上流に新しいフィールドが増えても、このクレートの更新を待たずに使えます。
- **「空」と「壊れた」を区別する。** 上流は存在しない leaderboard にも `200 []` を返すため、
  `allow_empty` を明示する設計にしています。
- **オフラインでテストできる。** `tests/fixtures` に実レスポンスを保存してあります。
  CI はネットワーク不要、`--ignored` のライブテストだけが API を叩きます。

## テスト

```console
# オフライン（CI と同じ）
cargo test

# 実 API に対する回帰テスト（1 スレッド推奨。クライアントが 1 req/秒に抑えるため）
cargo test --test live -- --ignored --test-threads=1 --nocapture
```

`tests/live.rs` の各テストは、上の表の 1 行に対応しています。

## マナー

- 対象は TETRA CHANNEL のみ。ゲーム本体の API (`https://tetr.io/api/`) は**使えません**（無断利用はアカウント停止対象）。
- 既定の `User-Agent` で名乗ります。変更する場合も識別可能な文字列にしてください。
- レートは 1 req/秒程度。`cache.cached_until` は尊重してください（既定で有効）。
- ページングを多用する場合は `TransportConfig::session_id` を設定してください（`X-Session-ID`）。

## ライセンス

MIT。[`LICENSE`](LICENSE) を参照してください。
