# 独自拡張タグ仕様

スクリプトの作成者が管理用の項目を定義し、利用アプリが指定した項目を Kit で読み取るための仕様です。対象は、スクリプト内の `SCRIPTMETA-BEGIN` から `SCRIPTMETA-END` までの項目部分です。Description 本文に書かれた同じ記述は本文として扱います。

SCRIPTMETAKit 2.1.0で指定読み取り・専用検証・編集を利用できます。完全な記述例は [メタデータ記述ガイド](MetadataAuthoring.md) を参照してください。

## 記述形式

```text
X-作成者ID-項目名=値
```

1 行に 1 項目を記述し、最初の半角 `=` でキーと値を分けます。Kit が新規作成・変更する行は `キー=値` の形式で出力します。

```text
SCRIPTMETA-BEGIN
Script-ID=org.iwashi.export
Name=書き出し
X-org.iwashi-Group=Production Tools
X-org.iwashi-Order=20
X-org.iwashi-URL=https://example.com/my%20scripts?Name=English&Version=2.0
X-org.iwashi-Note=
SCRIPTMETA-END
```

各項目は任意です。独自拡張タグを使うことを理由に、Name・Author・Tags・Meta-Default-language の条件を追加しません。言語選択には依存しない共通の文字列値として扱います。

## キー

| 部分 | 規則 |
| --- | --- |
| 接頭辞 | `X-`。独自拡張用に予約し、SCRIPTMETA の標準項目には使用しない |
| 作成者ID | ASCII 英数字とドット。標準表記は小文字。ドットで区切る各部分は 1 文字以上とし、先頭・末尾のドットと連続するドットを禁止する |
| 項目名 | ASCII 英数字とハイフン。先頭・末尾は英数字とする |

作成者IDと項目名の両方を必須とします。作成者IDには `iw` のような短い名前も、`org.iwashi` のような名前も使えます。作成者ID中の記号はドットだけです。

キーの照合は ASCII の大文字・小文字を区別しません。`X-` に続く最初のハイフンで作成者IDと項目名を分け、キー全体で照合します。キーの内部には空白・`:`・`=` を許可しません。

## 値

- 値は 1 行の文字列です。CR・LF を禁止します。
- 前後の空白は除き、内部の空白は保持します。空文字列も許可します。
- `:` と `=` を許可します。URL のクエリ等に含まれる記号も値の一部です。
- カンマを含む値も一つの文字列です。
- 引用符 `"` と `'`、バックスラッシュ、`%20` 等の表記は、値の文字として保持します。
- URL の空白は `%20` のようにエンコードして記述します。URL としての解釈や用途別の型変換は利用アプリが担当します。
- 保存先のコメント書式による制約も適用します。JavaScript／JSXの値に `*/`、AppleScriptの値に `(*` または `*)` がある場合は、内容を変えて保存せずエラーとします。URL中の該当文字は必要に応じてパーセントエンコードします。

例えば `X-org.iwashi-Order=0020` の値は文字列 `0020`、`X-org.iwashi-Group="Production Tools"` の値は引用符を含む文字列 `"Production Tools"` です。

## 予約済みのブロック境界文字列

次の文字列を、キーと値のいずれにも含めてはいけません。大文字・小文字を区別せず、文字列の一部に含まれる場合も禁止します。

```text
SCRIPTMETA-BEGIN
SCRIPTMETA-END
SCRIPTMETA-DIST-BEGIN
SCRIPTMETA-DIST-END
Description-BEGIN
Description-END
```

この制限は URL や引用符内にも適用します。

## 旧1.xで誤読される記述の禁止

独自項目の行全体に対し、旧1.xの既知項目の判定と同じ条件で検証します。行頭または空白文字の直後に次のキーが現れ、任意の空白を挟んで `=` または `:` が続く記述を禁止します。キーの照合は ASCII の大文字・小文字を区別しません。空白文字にはタブ等も含みます。

```text
Script-ID
Version
Description
Target-App
Min-Target-Version
Meta-URL
Latest-URL
Latest-Version
Latest-Page-URL
Name
Author
Release-Date
Edit-Password-SHA256
```

例えば次の記述は不適です。

```text
X-org.iwashi-Note=説明 Name=OtherName
X-org.iwashi-Note=説明 Name : OtherName
X-org.iwashi-Note="説明 Name=OtherName"
X-org.iwashi-Note='説明 Name=OtherName'
```

次の記述は適合します。

```text
X-org.iwashi-Name=CustomName
X-org.iwashi-Note=Name=Literal
X-org.iwashi-URL=https://example.com/script?Name=English&Version=2.0
```

入力確認と保存直前の確認は、実際に出力する `キー=値` の行に同じ検証を適用します。既存ファイルの互換性を調べるときは、値の前後の空白を除去する前の行に適用します。例えば `X-org.iwashi-Note= Name=OtherName` は旧1.xで誤読されるため、診断対象です。

## 指定読み取り

- 利用アプリは、取得したい完全なキー名の一覧、または全取得を表す `X-` をスキャン前に指定します。取得指定は省略でき、省略時と空の一覧では独自項目の値を通常の結果に保持しません。
- 指定に対応するキーだけを通常の解析と同時に抽出し、スクリプトごとの結果として返します。`X-` を指定した場合は、本仕様に適合するすべての独自項目を取得します。不適な項目は診断情報として扱います。
- 取得指定の有無にかかわらず、独自項目の行を項目境界として認識します。末尾カンマ付きの Tags に続く場合も、独自項目を Tags の続きとして取り込みません。
- Description 本文は、そのブロックの規則で読み取ります。
- 同じキーの複数記述は、記載順の文字列配列として返します。大文字・小文字だけが異なるキーも同じキーです。同じ値の複数記述も保持します。
- 項目が存在しない状態と、存在して値が空の状態を区別します。値が空の 1 項目は、空文字列を一つ含む配列として返します。
- 独自項目の内容を、Name・Author・Description 等の標準項目として再解釈しません。

| 取得指定 | 通常の結果に保持する独自項目 |
| --- | --- |
| 指定なし、または `[]` | なし |
| `["X-org.iwashi-Group", "X-org.iwashi-Order"]` | 指定した二つのキー |
| `["X-"]` | 仕様に適合するすべての独自項目 |

単独の `X-` は API の取得指定に用いる全取得の特別な記述です。ファイルに書くキーは、通常の `X-作成者ID-項目名` の形式に従います。個別取得は完全なキー名で照合します。取得指定の照合は ASCII の大文字・小文字を区別しません。`X-` と完全なキー名を同時に指定した場合は、全取得と同じ扱いとします。

取得指定は集合として扱い、並び順や大小文字、指定の重複だけの違いでは再読み込みを要求しません。全取得か個別取得かの区別と、正規化したキー集合をキャッシュの取得条件に含めます。全取得の指定に含まれる個別キーは、キャッシュの取得条件からも除きます。取得範囲を広げる場合は、必要な元ファイルの再読み込みで値を取得します。取得範囲を狭める場合は、保持済みの値を絞って結果とキャッシュを更新できます。全取得を選んだ場合のメモリとキャッシュには、その取得範囲の値を保持します。

値の意味、整数等への変換、並べ替えの規則は利用アプリが決めます。

## 検証とエラー

Kit は、ファイルを書き込まずにキーと値を検証できる公開 API を用意します。キーの構文、値の改行、予約境界文字列、旧1.xで誤読される記述を同じ共通処理で判定し、保存時にも適用します。保存先のコメント書式による制約は、書式を指定するrender／write APIでも検証します。

不適な入力に対しては、理由を表すコードと対象のキーを返します。ファイルの読み取り時には、取得可能な行番号も返します。アプリはこの結果を使い、入力欄へのエラー表示や保存操作の可否を決めます。

新規追加・変更する独自項目が不適なら、保存はエラーとします。既存ファイルの読み取りでは、不適な独自項目を通常の取得結果から除外し、診断情報を返します。メタデータのブロック構造を安全に特定できない場合は、メタデータ全体の読み取り・保存エラーとして扱います。

Swift と Windows 向けの公開 API は、同じ Kit の検証結果を利用します。アプリは、Order を整数にする、Status を所定の選択肢にする等、用途別の検証と表示メッセージのローカライズを担当します。

## 編集時の保全

取得指定は、保存する独自項目を限定する指示には使いません。元のファイルを使う編集では、未変更の独自項目と未知フィールドを保持します。取得対象外の独自項目も保持します。

既存の未変更の不適な独自項目は、診断対象として保持します。これにより、既存の `X-Custom=keep` のような未知項目も、他のメタデータの編集時に失われません。その項目を新規追加・変更する操作には、本仕様の検証を適用します。ブロック構造を安全に特定できない場合の保存エラーは、この保全より優先します。

既存の外部変更検知、バックアップ、条件付き保存を利用します。通常項目の必須条件と関係は [メタ項目の必須条件と関係](MetadataFieldRules.md) に従います。

## 実装API

返却する辞書のキーは、接頭辞を `X-`、その後を ASCII 小文字に正規化します。例えば `X-org.iwashi-Order` は `X-org.iwashi-order` です。未変更の行は、元のキーの表記とともに保存します。

Swift では、`customValues(for:)` で大小文字を区別せず取得できます。

```swift
let engine = ScriptMetaKitEngine()
try await engine.setPreferredLanguages(["ja", "en", "fr"])
try await engine.setCustomMetadataKeys(["X-"])
let result = try await engine.scan(folderURLs: folders, checkUpdates: false)
let values = result.allItems.first?.customValues(for: "X-org.iwashi-Order")

let metadata = try ScriptMetaKitMetadata.parse(
    source,
    preferredLanguages: ["ja", "en", "fr"],
    customMetadataKeys: ["X-org.iwashi-Group"]
)
let issues = try ScriptMetaKitMetadata.validateCustomMetadataField(
    key: "X-org.iwashi-URL",
    value: "https://example.com/?a=1&b=2"
)
```

編集用 draft の `customFields` はキー単位の更新指示です。指定を省いたキーは保全し、空配列でそのキーの全記述を削除し、空文字列を一つ含む配列で空の値を保存します。`setCustomValues(_:for:)` はキーを正規化して更新します。

```swift
let session = try await engine.beginScriptMetadataEditSession(fileURL: file)
var draft = session.draft
try draft.setCustomValues(["0030"], for: "X-org.iwashi-Order")
try draft.setCustomValues([""], for: "X-org.iwashi-Note")
try draft.setCustomValues([], for: "X-org.iwashi-Obsolete")
try await engine.commitScriptMetadataEditSession(
    session, draft: draft, backupRootURL: backups
)
```

編集用 draft は、キーの構文が有効な独自項目の全値を含みます。修正のため、不適な既存値も保持します。構文が不適なキーの行は `unknownLines` と元の本文で保全します。

Rust では `ParserOptions.custom_metadata_keys` に `CustomMetadataSelection` を設定し、Engine の `set_custom_metadata_keys` でも変更できます。結果は `MetadataPresentation.custom_fields`、編集用の更新指示は `ScriptMetadataDraft.custom_fields` です。`validate_custom_metadata_field` と `normalize_custom_metadata_key` を公開しています。

C ABI の追加関数は次のとおりです。既存の公開構造体のレイアウトは維持します。

- `smk_engine_set_custom_metadata_keys`
- `smk_metadata_parse_text_v2`
- `smk_validate_custom_metadata_field`
- `smk_normalize_custom_metadata_key`
- `smk_scan_result_custom_metadata_values`
- `smk_scan_result_custom_metadata_ranges`
- `smk_edit_result_custom_metadata_fields`
- `smk_engine_write_script_metadata_file_if_unchanged_v3`

通常の C 結果は、キーと値の共有配列と、一覧ごとの範囲で取得します。値は結果ハンドルの解放まで有効です。編集用の取得と更新は JSON のキー・文字列配列を利用します。旧来の保存APIから呼ばれた場合も、元の独自項目を保全します。

キャッシュスキーマは 4、メタデータの projection は 6 です。取得キーを読み取り条件に含め、旧 projection や異なる取得条件のキャッシュは再読み込みの対象にします。

## 互換性の確認範囲

公開タグ 1.0.5〜1.3.3 の16版について、macOSのApple Silicon上で各版のライブラリを使用しました。単純な値、ドット付きキー、空値、重複、URL、引用符、旧版で誤読される記述を含め、480件の読み取りと240件の保存を確認しました。各ケースの結果は全16版で一致しました。

2.1.0のmacOS Universal版とWindows x64版の検証結果は、リリース時の検証記録にまとめます。記述ガイドの完全な例も、Kitで解析し保存前後の意味が同じであることをテストします。
