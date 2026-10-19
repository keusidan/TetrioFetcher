# レート制限とキャッシュ

## API 側の要請

[TETRA CHANNEL API のドキュメント](https://tetr.io/about/api) にはこうあります。

> **Do not flood the API with requests.** … once a second should be fine for most cases,
> short bursts are OK.
>
> **Honor caching data.** If a response indicates its cache will expire after 10 minutes,
> please do not rerequest the data during that time, assuming you are sending an
> `X-Session-ID` header.
>
> **Send an `X-Session-ID` header** if you are often rerequesting the same datasets.

このクレートは、この3つを既定で守ります。

## スロットル（1 req/秒）

`TransportConfig::min_interval` の既定値は 1 秒です。
前回のリクエストから 1 秒経っていなければ、その分だけ待ってから送ります。

```rust
let f = Fetcher::new();           // min_interval = 1s

// 速くしたい場合（自己責任で。サーバーに優しくないので推奨しません）
use std::time::Duration;
use tetrio_fetcher::{Fetcher, TransportConfig};

let f = Fetcher::with_config(TransportConfig {
    min_interval: Duration::from_millis(250),
    ..Default::default()
});
```

**注意:** 短いバーストは許容されていますが、恒常的に速くすると
Cloudflare 側でブロックされる可能性があります。レートを上げるより、
キャッシュを効かせる方が実用的です。

## キャッシュ

応答には必ず `cache` ブロックが付きます。

```json
"cache": {"status": "miss", "cached_at": 1792440593328, "cached_until": 1792440893328}
```

`cached_until` までは内容が変わらないことが保証されているので、
その期間は通信せずに覚えた本文を返します。

```rust
let f = Fetcher::new();

f.server_stats()?;                 // 通信する
let s1 = f.transport().stats();
assert_eq!(s1.network_requests, 1);

f.server_stats()?;                 // 通信しない
let s2 = f.transport().stats();
assert_eq!(s2.network_requests, 1);
assert_eq!(s2.cache_hits, 1);
```

### 明示的に捨てる

```rust
f.transport().clear_cache();
```

### キャッシュを使わない

```rust
let f = Fetcher::with_config(TransportConfig {
    honour_server_cache: false,
    ..Default::default()
});
```

CLI では `--no-cache`。

### TTL の見方

```rust
let env = f.transport().get_envelope("general/stats", &[])?;
// CacheInfo { status, cached_at_ms, cached_until_ms }
```

## `X-Session-ID`

ページングを多用する場合、同じセッション ID を送ると
サーバー側の結果が一貫します（ページ境界でデータがずれない）。

```rust
let f = Fetcher::with_config(TransportConfig {
    session_id: Some("my-app-abc123".to_string()),
    ..Default::default()
});
```

- 関係ないリクエストに同じ ID を使わない、とドキュメントにあります
- **ブラウザからは設定できません**（CORS の許可ヘッダーに含まれていない）

## リトライ

| 応答 | リトライ |
|---|---|
| 接続断、TLS 失敗、タイムアウト | する |
| `429` | する |
| `5xx` | する |
| `404` / `422` | **しない**（答えなので） |

待ち時間は `backoff_base`（既定 400ms）から倍々で、最大 30 秒で頭打ち。
試行回数は `max_attempts`（既定 3）。

```rust
let f = Fetcher::with_config(TransportConfig {
    max_attempts: 5,
    backoff_base: Duration::from_millis(200),
    timeout: Duration::from_secs(10),
    ..Default::default()
});
```

## 統計

```rust
let s = f.transport().stats();
// TransportStats { network_requests, cache_hits, retries, non_json_responses }
```

CLI の `examples/summary.rs` は最後にこれを表示します。

## やらないこと

- 並列で叩きまくる（スロットルはクライアント単位なので、スレッドを分けると守られません）
- `min_interval` を極端に下げる
- ゲーム本体の API（`https://tetr.io/api/`）を叩く — **明示許可が必須**で、
  無断利用はアカウント停止対象です
