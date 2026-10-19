# テスト戦略

## 二段構え

```
cargo test                                    # オフライン（49 件）
cargo test --test live -- --ignored --test-threads=1   # 実 API（19 件）
```

**オフラインのテストが主**です。
ライブテストは `#[ignore]` してあり、`cargo test` では走りません。
これにより CI が第三者サービスに依存しません。

## オフライン: フィクスチャ

`tests/fixtures/` に実際の応答を保存してあります。

| ファイル | 何の罠か |
|---|---|
| `user.json` | 非公開ステータス（`-1`）と `null` の国 |
| `league_unranked.json` | `tr: -1`, `rank: "z"` |
| `league_ranked.json` | 全フィールドが揃った状態 |
| `user_leaderboard.json` | カーソルと隠しステータスの混在 |
| `records_40l.json` | `vsscore`、`clears.quads` の入れ子 |
| `league_ranks.json` | 二重入れ子（`data.data`） |
| `scoreflow_empty.json` | `startTime: 9007199254740991` |
| `stats.json` | 整数と浮動小数の混在 |
| `activity.json` | 数値の配列 |
| `html_404.html` | JSON の代わりに返る HTML |
| `err_422_leaderboard.json` | `leaderboard` is illegal |
| `err_404_user.json` | `No such user!` |

### なぜフィクスチャなのか

上流のデータは変わります。実際、同じユーザーの `gamesplayed` が
ある時は `-1`（非公開）、別の時は `34` でした。
テストを「値そのもの」で書くと毎日落ちます。

そこで、テストは**不変条件**で書いています。

```rust
// ✗ 値に依存する
assert_eq!(user.games_played, None);

// ✓ 不変条件
if let Some(games) = user.games_played {
    assert!(games >= 0, "負の値が漏れている");
}
```

## ライブテスト

`tests/live.rs` の各テストは、`API-Quirks` の 1 項目に対応しています。

```rust
#[test]
#[ignore = "network"]
fn pb_is_translated_to_progression() {
    let f = fetcher();
    for alias in ["pb", "pbs", "personal", "personalbest", "progression"] {
        let page = f.user_records("keusidan", "40l", alias, 5)
            .unwrap_or_else(|e| panic!("`{alias}` should be repaired, got {e}"));
        assert!(page.requested_limit <= 100);
    }
}
```

### `--test-threads=1` が必要な理由

クライアントが 1 req/秒にスロットルします。
並列に走らせると待ち時間が積み上がって遅くなるだけなので、1 スレッドで流します。

```bash
cargo test --test live -- --ignored --test-threads=1 --nocapture
```

## 何を検証しているか

| 分類 | 例 |
|---|---|
| 入力の修復 | `pb` → `progression`、`40l` → `40l_global`、大文字 → 小文字 |
| 入力の拒否 | `sprint` は通信前にエラー |
| センチネル | `-1` が漏れていない、`"z"` が漏れていない、JS MAX_SAFE_INTEGER |
| 型の揺れ | 整数の `xp` も浮動小数の `xp` も読める |
| エンベロープ | `success:false` が 200 でもエラーになる |
| HTML | 404 ページが `NonJson` になる |
| キャッシュ | 2 回目は通信しない、`clear_cache` で通信する |
| ページング | カーソルが往復し、ページ間で重複しない |

## 追加するとき

1. **ライブで再現**する（`curl` で確認）
2. 応答を `tests/fixtures/` に保存する
3. `fixtures_test.rs` に不変条件を書く
4. `live.rs` に対応するテストを足す（`#[ignore = "network"]`）
5. `docs/api-quirks.md` に再現手順を書く

## CI

`.github/workflows/ci.yml` で:

- Ubuntu / macOS / Windows でビルドとテスト
- `cargo fmt --check` と `clippy -D warnings`
- MSRV（1.85.0）でのビルド
- ライブテストは手動実行のみ（`workflow_dispatch`）
