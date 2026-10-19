# API の落とし穴

すべて 2026-10-19 に実際に API を叩いて確認した結果です。
再現用の `curl` と、このクレートでの扱いを並べています。

Base: `https://ch.tetr.io/api/`
Docs: <https://tetr.io/about/api>

---

## 1. `pb` という leaderboard は存在しない

`/users/:user/records/:gamemode/:leaderboard` の `:leaderboard` は
`top` / `recent` / `progression` の 3 つだけです。

```console
$ curl -s "https://ch.tetr.io/api/users/keusidan/records/40l/pb"
{"success":false,"error":{"msg":"The params field 'leaderboard' is illegal","key":"leaderboard","context":"params"}}

$ curl -s "https://ch.tetr.io/api/users/keusidan/records/40l/progression"
{"success":true,"data":{"entries":[]},...}
```

他クライアントが `pb` / `global` / `personal` と呼んでいるものをそのまま渡すと落ちます。

**扱い:** `PersonalBoard::parse` で変換。

| 入力 | 変換先 |
|---|---|
| `pb`, `pbs`, `personal`, `personalbest` | `progression` |
| `global`, `country`, `all`, `best` | `top` |
| `latest`, `new`, `history` | `recent` |

---

## 2. `200` + 空配列はエラーではない

```console
$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/records/40l"
{"success":true,"data":{"entries":[]},...} [200]

$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/records/99l_global"
{"success":true,"data":{"entries":[]},...} [200]

$ curl -s -w ' [%{http_code}]\n' "https://ch.tetr.io/api/users/keusidan/records/foo/top"
{"success":true,"data":{"entries":[]},...} [200]
```

`/records/:leaderboard` は `<mode>_global` / `<mode>_country_XX` / `...@<revolution>` の
合成 ID を要求します。単に `40l` を渡すと「空」が返り、gamemode の打ち間違いも同じ「空」になります。

**扱い:**
- `RecordLeaderboard::parse` で `40l` → `40l_global`、`40l_JP` → `40l_country_JP`
- gamemode の誤りは送信前に `Error::Invalid`（通信しない）

---

## 3. 大文字はユーザー不在

```console
$ for u in KEUSIDAN Keusidan keusidan; do
    curl -s -o /dev/null -w "$u -> %{http_code}\n" "https://ch.tetr.io/api/users/$u"; done
KEUSIDAN -> 404
Keusidan -> 404
keusidan -> 200
```

**扱い:** `normalize_user` で小文字化。MongoID は元から小文字なので影響なし。

---

## 4. 404 が HTML で返る

```console
$ curl -s -o /dev/null -w "%{http_code} %{content_type}\n" \
    "https://ch.tetr.io/api/users/keusidan/records/40l"
404 text/html; charset=utf-8
```

中身は TETRA CHANNEL の 404 ページ（`<!DOCTYPE html>` から始まる約 2.9KB）。
JSON パーサに渡すと `expected value at line 1 column 1` になります。

**扱い:** `Error::NonJson { url, content_type, snippet }`。
snippet に先頭 120 文字を入れて、何が返ってきたか分かるようにしています。

実例:

```
error: expected JSON from https://ch.tetr.io/api/users/keusidan/records/40l
       but got text/html; charset=utf-8 (first bytes: "<!DOCTYPE html>  <html> ...")
```

---

## 5. `200 OK` なのに失敗

`success: false` は 422 / 404 のどちらでも返り、HTTP ステータスだけ見ると「成功」に見えます。

**扱い:** 必ずエンベロープを展開し `success` で分岐。
❌ HTTP ステータスで判断しない。

---

## 6. 数値の型が一定しない

```jsonc
// /users/keusidan
{"xp": 22336}                    // 整数
// /users/by/league
{"xp": 68966508.61132418}        // 浮動小数
```

`serde` で `i64` に固定すると片方で deserialize が落ちます。

**扱い:** `jval::as_i64` は `42` / `42.0` / `"42"` をすべて受けます。

---

## 7. `-1` は「非公開」、`0` は「0」

`gamesplayed` / `gameswon` / `gametime` は、非公開設定だと `-1` になります。

```
-1   -> 非公開  ->  None
 0   -> 本当に 0 ->  0 のまま
```

この2つを潰すと「非公開」が「未プレイ」に化けます。

**扱い:** `jval::as_stat` / `as_stat_f64`。

---

## 8. 未ランクは `"z"` と `-1`

`rank: "z"`, `tr: -1`, `standing: -1`, `percentile: -1`。

`"z"` を階級として通すと、UI に「Z ランク」と表示されてしまいます。

**扱い:** `jval::as_rank` / `as_rating` → `None`。

---

## 9. `9007199254740991` という日付

`/labs/scoreflow` が空のときの `startTime` は JS の `Number.MAX_SAFE_INTEGER`。
ミリ秒として解釈すると西暦 **285428751 年**になります。

**扱い:** `jval::as_millis` がこの値だけ `None` にします。

---

## 10. 同じ意味で 2 つの名前

`results.aggregatestats` は `vsscore` ですが、他の場所では `vs` です。

**扱い:** 正規化後は常に `vs`（`AggregateStats::vs`）。

---

## 11. クエリの検証エラーが細かすぎる

```
?limit=0      -> "The query field 'limit' must be at least 1"
?limit=999    -> "The query field 'limit' must be no more than 100"
?limit=abc    -> "The query field 'limit' must be an integer"
?after=..&before=.. -> "The query fields 'before' and 'after' may not be combined"
```

**扱い:** `limit` は `1..=100` に丸める。`before`/`after` は `PageDirection` で一方だけ送る。

---

## 12. 入れ子が1段深いエンドポイント

`/labs/league_ranks` は `data._id` と `data.data.{...}` の二重構造で、他と形が違います。

**扱い:** `parse_league_ranks` が展開し、`position` 順に並べ替えて返します。

---

## 13. 検索クエリは「コロン2個」が必須

```console
$ curl -s "…/users/search/keusi"
{"error":{"msg":"The params field 'query' must contain two colons.",...}}
```

正しい形は `social:type:value`（例 `discord:id:1234`）。

**扱い:** `search_users("discord:1234")` は `discord:id:1234` に補完。
素の文字列は補完できないので、API の返答をそのままエラーとして返します。

---

## 14. ブラウザからは直接叩けない

```
access-control-allow-origin: https://tetr.io
access-control-allow-headers: Authorization, Content-Type
```

`X-Session-ID` は許可ヘッダーに含まれていません。
ブラウザから使う場合はプロキシが必要です（このクレートはネイティブ / サーバ用途）。

---

## 15. レート制限とデータの揺れ

12 連続リクエストでは 429 は出ませんでした（Cloudflare 経由）。
ただしドキュメントは 1 req/秒程度と `cached_until` の尊重を求めています。

また、同じユーザーでもある時点で `gamesplayed: -1`（非公開）、
別の時点で `34` が返ることがありました。
**フィクスチャはその瞬間のスナップショット**なので、テストは
「負の値が漏れていないこと」のような不変条件で書いています。

---

## 自分で確かめる

```bash
# 1. 存在しない leaderboard 名
curl -s "https://ch.tetr.io/api/users/keusidan/records/40l/pb"

# 2. 大文字ユーザー
curl -s -o /dev/null -w "%{http_code}\n" "https://ch.tetr.io/api/users/KEUSIDAN"

# 3. HTML の 404
curl -s -o /dev/null -w "%{content_type}\n" "https://ch.tetr.io/api/users/keusidan/records/40l"

# 4. limit の検証
curl -s "https://ch.tetr.io/api/records/40l_global?limit=999"
```

同じ `curl` を `tfetch` に通すと、どれも型付きの結果になります。
