# データ正規化

## 出力は「上流の写し」ではない

`model.rs` の型は、上流の形をそのまま写したものではありません。
利用者が毎回書かなくて済むように、次の3つを済ませてから返します。

1. **名前を揃える** — `_id` / `id` の混在、`vsscore` / `vs` の重複を解消
2. **センチネルを解決する** — `-1`、`"z"`、JS の `MAX_SAFE_INTEGER` を `None` に
3. **派生値を計算する** — APP、平均 PPS/KPS

## 隠しステータス

上流:

```json
{"gamesplayed": -1, "gameswon": 11574, "gametime": 10481869.44}
```

正規化後:

```json
{"games_played": null, "games_won": 11574, "gametime_seconds": 10481869.44}
```

`games_played` が `null` なのは「非公開」だからで、「0 試合」ではありません。
`0` は `0` のまま通ります。

## 未ランク

上流:

```json
{"tr": -1, "rank": "z", "gxe": -1, "glicko": -1, "rd": -1, "standing": -1}
```

正規化後:

```json
{"tr": null, "rank": null, "gxe": null, "glicko": null, "rd": null}
```

`"z"` を文字列のまま通すと、UI に「Z ランク」と出てしまいます。

## タイムスタンプ

上流（空のスコアフロー）:

```json
{"startTime": 9007199254740991, "points": []}
```

正規化後:

```json
{"start_time": null, "points": []}
```

ミリ秒として解釈すると西暦 285428751 年になります。
この値は JS の `Number.MAX_SAFE_INTEGER` で、「データなし」の意味です。

## 数値の型

| フィールド | 上流 | 出力 |
|---|---|---|
| `xp` | `22336` または `68966508.61` | `f64` |
| `gamesplayed` | `-1` / `0` / `34` | `Option<i64>` |
| `gametime` | `7777.294` | `Option<f64>` |
| `tr` | `-1` / `24863.06` | `Option<f64>` |
| `supporter` | `false` / `"false"` / `0` | `bool` |

## 派生値

### APP（Attack Per Piece）

上流にはありませんが、プレイヤーがよく見る値です。

```rust
let stats = league.recent_stats().unwrap();
let app = stats.app();  // Some(apm / pps / 60)
```

`pps` が 0 のときは `None`（ゼロ除算を避ける）。

### サーバー全体の PPS / KPS

```rust
let stats = fetcher.server_stats()?;
stats.average_pps;  // Some(piecesplaced / gametime)
stats.average_kps;  // Some(inputs / gametime)
```

上流のサイトも同じ計算をクライアント側でしているので、
どこで計算しても同じ値になります。ならば1箇所で。

## 生データを捨てない

正規化していない部分も残してあります。

```rust
// レコードの results 全体
let raw = record.raw_results;   // Option<Value>

// ゲームの stats 全体
let raw_stats = record.stats.raw; // Option<Value>

// 任意のエンドポイント
let anything = fetcher.raw_get("users/keusidan", &[])?;
```

上流にフィールドが増えても、このクレートの更新を待たずに使えます。

## ページング

上流の `p` は `{pri, sec, ter}` のオブジェクトです。

```json
{"p": {"pri": 24863.062055852624, "sec": 0, "ter": 0}}
```

出力では `"pri:sec:ter"` の文字列にし、そのまま `after` / `before` に渡せます。

```rust
let cursor = page.next_cursor;  // Some("24863.062055852624:0:0")
```
