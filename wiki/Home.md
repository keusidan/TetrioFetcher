# TetrioFetcher Wiki

TETR.IO の TETRA CHANNEL API を扱う Rust クライアント、**TetrioFetcher** のドキュメントです。

| | |
|---|---|
| リポジトリ | https://github.com/keusidan/TetrioFetcher |
| クレート名 | `tetrio_fetcher` |
| ライセンス | MIT |

## このプロジェクトの主張

> **TETRA CHANNEL API は、同じ「失敗」を 5 通り以上の形で返す。**
> 素直に書いたクライアントは、そのすべてを自分で処理することになる。

TetrioFetcher は、その処理を引き受けます。
この Wiki には「何を・なぜ・どう吸収しているか」を、
実際に API を叩いて確認した結果とあわせて置いてあります。

## 30 秒でわかる例

```rust
use tetrio_fetcher::Fetcher;

let fetcher = Fetcher::new();

// 1. 大文字でも通る（API 単体は 404 を返す）
let user = fetcher.user("Keusidan")?;

// 2. 非公開の項目は None（API は -1 を返す）
assert_eq!(user.games_played, None);

// 3. `pb` は存在しない leaderboard 名だが、変換される
let pbs = fetcher.user_records("keusidan", "40l", "pb", 10)?;
```

## ページ一覧

### 使う

| ページ | 内容 |
|---|---|
| [はじめに](Getting-Started) | インストール、最初のリクエスト、CLI |
| [エラーハンドリング](Error-Handling) | 型付きエラーの使い分け |

### 理解する

| ページ | 内容 |
|---|---|
| [API の落とし穴](API-Quirks) | 実際に確認した 15 の挙動と再現用 `curl` |
| [アーキテクチャ](Architecture) | レイヤ構成と、なぜ同期なのか |
| [データ正規化](Data-Normalization) | `-1` / `"z"` / 型揺れをどう扱うか |
| [レート制限とキャッシュ](Rate-Limits-and-Caching) | 1 req/秒と `cached_until` の尊重 |

### 開発する

| ページ | 内容 |
|---|---|
| [テスト戦略](Testing) | オフラインとライブの二段構え |
| [FAQ](FAQ) | よくある質問 |

## 免責

TETR.IO の関係者の方々とは一切関係ありません。
**このライブラリの問題を TETR.IO 側へ報告しないでください。**

利用対象は [TETRA CHANNEL API](https://tetr.io/about/api) のみです。
ゲーム本体の API（`https://tetr.io/api/`）は、明示的な許可なく使うことはできません。
