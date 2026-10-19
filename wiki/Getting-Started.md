# はじめに

## インストール

まだ crates.io には公開していないので、git から引きます。

```toml
[dependencies]
tetrio-fetcher = { git = "https://github.com/keusidan/TetrioFetcher" }
```

## 最初のリクエスト

```rust
use tetrio_fetcher::Fetcher;

fn main() -> tetrio_fetcher::Result<()> {
    let fetcher = Fetcher::new();

    let user = fetcher.user("Keusidan")?;   // 大文字でも OK
    println!("{} ({} XP)", user.username, user.xp);

    Ok(())
}
```

`Fetcher::new()` は次の設定で始まります。

| 設定 | 既定値 | 理由 |
|---|---|---|
| レート | 1 req/秒 | API ドキュメントの要請 |
| リトライ | 3 回（指数バックオフ） | 429 / 5xx / 切断対策 |
| キャッシュ | `cache.cached_until` を尊重 | 二重取得を避ける |
| User-Agent | `TetrioFetcher/0.1.0 (+GitHub URL)` | ボットは名乗る |

## よく使う呼び出し

### ユーザー情報

```rust
let user = fetcher.user("keusidan")?;

// 非公開設定の項目は None。0 でも -1 でもありません。
match user.games_played {
    Some(games) => println!("{games} 試合"),
    None => println!("非公開"),
}
```

### リーグ

```rust
let league = fetcher.user_league("keusidan")?;
// `rank` は Option<String> なので、move せずに借用する
match (league.rank.as_deref(), league.tr) {
    (Some(rank), Some(tr)) => println!("{} ({tr:.0} TR)", rank.to_uppercase()),
    _ => println!("未ランク"),   // API は "z" と -1 を返すが、None になる
}
```

### 自己ベスト

```rust
// `pb` は存在しない leaderboard 名ですが、`progression` に変換されます。
let pbs = fetcher.user_records("keusidan", "40l", "pb", 10)?;
for pb in &pbs.entries {
    println!("{:?} 点 / {} 行", pb.stats.score, pb.stats.lines.unwrap_or(0));
}
```

### ランキング

```rust
// `40l` だけでも通ります（内部で 40l_global に展開）。
let top = fetcher.records("40l", 5)?;

// 国別
let jp = fetcher.user_leaderboard("league", Some("JP"), 10)?;
```

### ページング

```rust
use tetrio_fetcher::PageDirection;

let page1 = fetcher.user_leaderboard("league", None, 25)?;
if let Some(cursor) = &page1.next_cursor {
    let page2 = fetcher.user_leaderboard_opts(
        "league", None, 25,
        Some((cursor, PageDirection::Down)),
        true,
    )?;
    assert_eq!(page2.entries[0].placement, 1);  // ページごとに 1 から
}
```

カーソルは API の `p.{pri,sec,ter}` を `"pri:sec:ter"` にしたものです。
`before` / `after` は排他なので、`PageDirection` でどちらか一方だけを送ります。

## CLI

```bash
cargo install --path .

tfetch stats
tfetch user Keusidan
tfetch user-records keusidan 40l pb --limit 3
tfetch leaderboard league --country JP --limit 5
tfetch records 40l --limit 3
tfetch ranks
tfetch raw general/activity
```

出力は整形済み JSON です。エラーは 1 行で stderr に出て、終了コード 1 を返します。

```console
$ tfetch user nobody
error: TETRA CHANNEL API error (404): No such user! | Either you mistyped something, or the account no longer exists.
```

### フラグ

| フラグ | 意味 |
|---|---|
| `--limit N` | 取得件数（1..=100 に丸められる） |
| `--country CC` | ISO 3166-1 国コードで絞る |
| `--base URL` | 別ホスト（プロキシ・テスト用） |
| `--no-cache` | サーバーキャッシュを使わず毎回取得 |

## 次のページ

- [API の落とし穴](API-Quirks) — 何を吸収しているのか
- [アーキテクチャ](Architecture) — 中の構造
