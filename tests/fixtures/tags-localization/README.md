# タグ・ローカライズの JSX テストファイル

このフォルダの 10 個の JSX は、開発中の SCRIPTMETA 新仕様を確認するためのテストデータです。実用スクリプトや配布用サンプルとしては扱いません。

全ファイルに以下を設定しています。

- テスト用の一意な Script-ID: `com.example.scriptmetakit.test.tags01` ～ `tags10`
- テストデータの版: `Version=0.0.1`（Kit のバージョン番号ではありません）
- 名前の先頭: `[TEST 01]` ～ `[TEST 10]`
- 共通タグ: `test,fixture`
- 素の `Name` と `Meta-Default-language`
- ダミー本体: テスト識別文字列を返すだけ。文書・ファイル操作やアプリの API 呼び出しはありません。

スクリプトの実行は不要です。このフォルダを Kit のスキャン対象に指定して確認します。優先言語は、呼び出す側から `ja → en → fr` の順で渡してください。

| ファイル | 確認内容 | 選択言語: Name / Author / Description | 期待するタグ（出現順） |
|---|---|---|---|
| 01-basic-tags.jsx | 基本のタグ、無ラベルの既定言語説明と英語・フランス語の説明 | ja / ja / ja | test, fixture, 画像, image |
| 02-tag-whitespace.jsx | タグの前後の空白除去、内部空白の保持、無ラベルの日本語説明の後に英語 | ja / ja / ja | test, fixture, 画像, image, batch export |
| 03-multiline-tags.jsx | カンマからの継続行、次の項目で終了 | ja / ja / ja | test, fixture, 画像, image, export, print |
| 04-repeated-tags.jsx | 複数の Tags を結合、完全一致だけ重複除去、空の Tags | ja / ja / ja | test, fixture, 画像, image, print, Image, export |
| 05-trailing-comma.jsx | 空要素と末尾カンマを無視 | ja / ja / ja | test, fixture, layout, print |
| 06-english-selection.jsx | 日本語がない場合は次の英語を選択 | en / en / en | test, fixture, export, english |
| 07-french-selection.jsx | 日本語・英語がない場合はフランス語を選択 | fr / fr / fr | test, fixture, layout, french |
| 08-default-language.jsx | 優先言語がすべてない場合は既定のドイツ語へ戻る | de / de / de | test, fixture, default, german |
| 09-mixed-field-languages.jsx | 項目ごとに選択言語が異なる | en / de / fr | test, fixture, mixed, translation |
| 10-description-escapes.jsx | 角括弧・バックスラッシュのエスケープ、説明内の Tags は本文 | ja / ja / ja | test, fixture, escape, documentation |

言語を指定しない場合、01～05・10 は日本語、06～09 はドイツ語になります。タグは言語を切り替えても変わりません。

01・02 はユーザーが変更した状態を保っています。`Meta-Default-language=ja` があるため、Description冒頭の `[ja]` は省略できます。英語指定では、01 は英語の段落だけ、02 は `this is the test.` だけを返します。

10 の日本語説明では、`\[en\]` が `[en]`、`C:\\Test\\fixtures` が `C:\Test\fixtures` として返ります。`example-only` と `not-a-real-tag` は実際のタグ一覧には入りません。

既存の読み取り用サンプルでの確認コマンド（Kit のルートで実行）:

```sh
cargo run --locked --offline --target aarch64-apple-darwin --example localized_scan -- tests/fixtures/tags-localization
```

このサンプルは優先言語 `ja, en, fr` を明示してスキャン結果を JSON で返します。スクリプト本体の実行や更新確認は行いません。

2026-11-02 に Apple Silicon 上の Kit で、ユーザーが変更した 01・02 を含む 10 件すべてを読み取り、表のタグ一覧と選択言語が一致することを確認済みです。言語未指定・キャッシュ再利用・言語順序変更・英語のみ・フランス語のみを含む 8 条件で、読み込みエラーと診断情報はともに 0 件でした。
