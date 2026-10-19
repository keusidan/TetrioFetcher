# TETRA CHANNEL API — 確認した挙動と、その扱い

すべて 2026-10-19 に実際に API を叩いて確認した結果です。
再現用の `curl` と、このクレートでの扱いを並べています。

Base: `https://ch.tetr.io/api/`
Docs: https://tetr.io/about/api

## 1. ルート名のずれ — `pb` は存在しない

`/users/:user/records/:gamemode/:leaderboard` の `:leaderboard` は
`top` / `recent` / `progression` の 3 つだけです。

```
$ curl -s "https://ch.tetr.io/api/users/keusidan/records/40l/pb"
{"success":false,"error":{"msg":"The params field 'leaderboard' is illegal","key":"leaderboard","context":"params"}}

$ curl -s "https://ch.tetr.io/api/users/keusidan/records/40l/progression"
{"success":true,"data":{"entries":[]},...}
```

他クライアントが `pb` / `global` / `personal` と呼んでいるものを
そのまま渡すとここで落ちます。→ `PersonalBoard::parse` で変換。

## 2. `200` + 空配列はエラーではない

```
$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/records/40l"
{"success":true,"data":{"entries":[]},...} [200]

$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/records/99l_global"
{"success":true,"data":{"entries":[]},...} [200]

$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/users/keusidan/records/foo/top"
{"success":true,"data":{"entries":[]},...} [200]
```

`/records/:leaderboard` は `<mode>_global` / `<mode>_country_XX` / `...@<revolution>` の
合成 ID を要求します（例 `40l_global`, `blitz_country_XM`, `zenith_global@2024w31`）。
単に `40l` を渡すと「空」が返り、gamemode の打ち間違いも同じ「空」になります。

- `RecordLeaderboard::parse` で `40l` → `40l_global`、`40l_JP` → `40l_country_JP`
- gamemode の誤りは送信前に `Error::Invalid`

## 3. 大文字はユーザー不在

```
$ for u in KEUSIDAN Keusidan keusidan; do
    curl -s -o /dev/null -w "$u -> %{http_code}\n" "https://ch.tetr.io/api/users/$u"; done
KEUSIDAN -> 404
Keusidan -> 404
keusidan -> 200
```

→ `routes::normalize_user` で小文字化。

## 4. 404 が HTML で返る

```
$ curl -s -o /dev/null -w "%{http_code} %{content_type}\n" \
    "https://ch.tetr.io/api/users/keusidan/records/40l"
404 text/html; charset=utf-8
```

しかも中身は TETRA CHANNEL の 404 ページ（`<!DOCTYPE html>` で始まる約 2.9KB）。
JSON パーサに渡すと `expected value at line 1 column 1` になります。
→ `Error::NonJson { url, content_type, snippet }`。

## 5. `200 OK` なのに失敗

`success: false` は 422/404 のどちらでも返り、HTTP ステータスだけを見ると
「成功」に見えることがあります。→ 必ずエンベロープを展開し、`success` で分岐。

## 6. 数値の型が一定しない

```
// /users/keusidan
{"xp": 22336}                    // 整数
// /users/by/league
{"xp": 68966508.61132418}        // 浮動小数
```

`serde` で `i64` に固定すると、片方で deserialize が落ちます。
→ `jval::as_i64` は `42` / `42.0` / `"42"` を受けます。

## 7. `-1` は「非公開」、`0` は「0」

`gamesplayed` / `gameswon` / `gametime` は、非公開設定だと `-1` になります。
`rank: "z"`, `tr: -1`, `standing: -1`, `percentile: -1` は未ランクです。

```
-1   -> 非公開 / 未ランク  ->  None
 0   -> 本当に 0          ->  0 のまま
```

→ `jval::as_stat` / `as_rank` / `as_rating`。

## 8. `9007199254740991` という日付

`/labs/scoreflow` が空のときの `startTime` は JS の `Number.MAX_SAFE_INTEGER`。
ミリ秒として解釈すると西暦 285428751 年になります。→ `jval::as_millis` が `None` に。

## 9. 同じ意味で 2 つの名前

`results.aggregatestats` は `vsscore` ですが、他の場所では `vs` です。
→ 正規化後は常に `vs`。

## 10. クエリの検証エラーが細かすぎる

```
?limit=0      -> "The query field 'limit' must be at least 1"
?limit=999    -> "The query field 'limit' must be no more than 100"
?limit=abc    -> "The query field 'limit' must be an integer"
?after=..&before=.. -> "The query fields 'before' and 'after' may not be combined"
```

→ `limit` は `1..=100` に丸める。`before`/`after` は `PageDirection` で一方だけを送る。

## 11. 入れ子が 1 段深いエンドポイント

`/labs/league_ranks` は `data._id` と `data.data.{...}` の二重構造で、他のエンドポイントと形が違います。
→ `parse_league_ranks` が展開し、`position` 順に並べ替えます。

## 12. 検索クエリは「コロン 2 個」が必須

```
$ curl -s "…/users/search/keusi"
{"error":{"msg":"The params field 'query' must contain two colons.",...}}
```

正しい形は `social:type:value`（例 `discord:id:1234`）。
→ `search_users("discord:1234")` は `discord:id:1234` に補完します。
素の文字列は補完できないため、API の返答をそのままエラーとして返します。

## 13. ブラウザからは直接叩けない

```
access-control-allow-origin: https://tetr.io
access-control-allow-headers: Authorization, Content-Type
```

`X-Session-ID` は許可ヘッダーに含まれていません。
ブラウザから使う場合はプロキシが必要です（このクレートはネイティブ / サーバ用途）。

## 14. レート制限

12 連続リクエストでは 429 は出ませんでした（Cloudflare 経由）。
ただしドキュメントは 1 req/秒程度と `cached_until` の尊重を求めています。
→ `min_interval = 1s`、`cache.cached_until` を尊重（いずれも既定で有効）。

## 15. データが「揺れる」

同じユーザーでも、ある時点で `gamesplayed: -1`（非公開）、別の時点で `34` が返ることがありました。
保存したフィクスチャはその瞬間のスナップショットなので、
テストは「負の値が漏れていないこと」のような不変条件で書いています。
