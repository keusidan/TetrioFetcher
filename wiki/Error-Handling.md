# エラーハンドリング

## すべての失敗が 1 つの型に入る

```rust
pub enum Error {
    Api { status, msg, key, context },   // 構造化された API エラー
    NonJson { url, content_type, snippet }, // JSON でない応答
    Malformed { url, detail },           // エンベロープの形が違う
    EmptyResult { url },                 // 200 だが中身が空
    Transport { attempts, source_msg },  // ネットワーク
    Unavailable { status, attempts },    // 5xx / 429 がリトライ後も続く
    Invalid { field, value, reason },    // 送信前に拒否
}
```

## どの失敗が、どこから来るか

| 状況 | バリアント | リトライ |
|---|---|---|
| `pb` など存在しない leaderboard | `Invalid` | しない（通信していない） |
| gamemode の打ち間違い | `Invalid` | しない |
| 存在しないユーザー | `Api { status: 404 }` | しない |
| サーバーがパラメータを拒否 | `Api { status: 422, key }` | しない |
| HTML の 404 ページ | `NonJson` | しない |
| `success` フィールドが無い | `Malformed` | しない |
| 接続断・TLS 失敗 | `Transport` | する |
| Cloudflare 5xx | `Unavailable` | する |

## 使い分けの例

### 「ユーザーがいない」と「通信できない」を区別する

```rust
match fetcher.user("keusidan") {
    Ok(user) => println!("{}", user.username),

    // 入力の誤り。ユーザーに「名前を確認して」と言えばよい
    Err(err) if err.is_likely_user_mistake() => {
        eprintln!("そのプレイヤーはいません: {err}");
    }

    // 一時的な障害。後で再試行すればよい
    Err(err) if err.is_retryable() => {
        eprintln!("一時的なエラー: {err}");
    }

    Err(err) => eprintln!("予期しないエラー: {err}"),
}
```

### 空をエラーとして扱う

API は存在しない leaderboard にも `200 []` を返すため、
「空」が「間違い」なのか「本当に無い」のか分かりません。
既定は空のまま返し、厳しくしたいときは `allow_empty = false` にします。

```rust
// 空でも Ok(空の Page)
let page = fetcher.records("40l", 10)?;

// 空なら Error::EmptyResult
let strict = fetcher.records_opts("40l", 10, None, false);
assert!(matches!(strict, Err(tetrio_fetcher::Error::EmptyResult { .. })));
```

### サーバーが指摘したフィールドを知る

```rust
if let Err(err) = fetcher.user_records("keusidan", "40l", "unknown", 5) {
    // サーバーが key を教えてくれた場合
    if let Some(key) = err.offending_key() {
        eprintln!("`{key}` が拒否されました");
    }
}
```

※ このクレートは普通に使う限り、既知の誤りは送信前に直すので
`Api { key }` を見ることはほとんどありません。新しいエンドポイントを
`raw_get` で叩いたときなどに役立ちます。

## 表示

`Display` を実装済みなので、そのまま `eprintln!` できます。

```console
error: TETRA CHANNEL API error (404): No such user! | Either you mistyped something, or the account no longer exists.
error: invalid gamemode: "sprint" (expected one of 40l, blitz, zenith, zenithex, league)
error: expected JSON from https://… but got text/html; charset=utf-8 (first bytes: "<!DOCTYPE html> …")
error: empty result from https://… (the API returns 200 + [] for unknown ids)
```

`key` と `context` があれば末尾に付きます。

```
TETRA CHANNEL API error (422): The params field 'leaderboard' is illegal [params.leaderboard]
```

## ヘルパ

| メソッド | 意味 |
|---|---|
| `is_retryable()` | 同じリクエストをやり直す価値がある |
| `is_likely_user_mistake()` | 入力の誤りの可能性が高い |
| `offending_key()` | サーバーが指摘したフィールド名 |
