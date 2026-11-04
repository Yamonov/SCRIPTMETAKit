# SCRIPTMETA 2.1 記述ガイド

この文書は、スクリプトの作成者と、作成・編集を行うAIのための記述仕様です。SCRIPTMETAKit 2.1.0を対象とします。

## 記述する場所と基本形

メタデータはスクリプト先頭付近のコメント内に置きます。Kitの通常設定は先頭128 KiBまでを読むため、その範囲内にメタデータ全体を収めます。JavaScript／JSXでは `/* ... */`、AppleScriptでは `(* ... *)` のコメントを使います。

ブロック境界と項目名は、以下の例の表記を使います。区切りには半角 `=` と半角カンマを使い、通常の項目は1行に一つ記述します。メタデータの値や説明にも、コメントの終了記号を直接埋め込まないようにします。Kitの保存APIは通常の本文をコメントに安全な表記へ調整し、X値については後述の保存エラー条件を適用します。

```jsx
/*
SCRIPTMETA-BEGIN
Script-ID=org.example.export-images
Name=選択した画像を書き出す
Version=1.0.0
Author=Example Team
Target-App=Illustrator
Tags=image,export,画像
Description-BEGIN
選択した画像を書き出します。
Description-END
SCRIPTMETA-END
*/
```

説明が不明な場合は、既存の空欄・省略を保ちます。作者名・ID・URL等には依頼者が指定した情報を使います。既存ファイルを編集する際は、スクリプト本体、未知の項目、依頼対象外の翻訳を保全します。

| 項目 | 記述条件 |
| --- | --- |
| `Script-ID` | 必須。空白だけの値は不可。スクリプトを識別する安定した値を指定する |
| 素の `Name` | 新規作成では内容を表す名前を記述する。多言語形式では必須。従来形式の保存では互換性のため省略可能 |
| 素の `Author` | 任意。編集パスワードを指定する場合は必須 |
| `Version` | 任意。スクリプト自身のバージョン。例 `1.0.0` |
| `Target-App` | 任意。対象アプリ名。例 `Illustrator` |
| `Description-BEGIN`／`Description-END` | 任意。複数行の説明を囲む。空の説明も許可する |
| `Meta-Default-language` | 通常形式では任意。多言語形式では必須 |
| `Tags` | 任意。他項目の必須条件とは独立 |
| `X-作成者ID-項目名` | 任意。作成者が管理用に定義する共通項目 |

`Min-Target-Version`、`Meta-URL`、`Release-Date`、`Edit-Password-SHA256` 等の条件は [メタ項目の必須条件と関係](MetadataFieldRules.md) に従います。編集パスワードのハッシュはKitのAPIで生成します。

## Name・Author・Descriptionの多言語記述

多言語化できるのはName・Author・Descriptionの3項目です。言語コードはKitが検証するBCP 47を使います。日本語は `ja`、英語は `en`、フランス語は `fr` です。地域を含む例は `ja-JP`、`en-US` です。

`Meta-Default-language` は素のName・Authorと既定の説明の言語を示します。素のNameは必ず一つ記述します。追加のNameとAuthorには `Name[en]`、`Author[en]` のような言語付きキーを使います。

Descriptionは一つの `Description-BEGIN`／`Description-END` 内で言語を分けます。行頭の `[ja]` 等から、次の有効な言語ラベルの行頭、またはDescription-ENDまでがその言語の本文です。ラベル直後に本文を続けても、次の行から本文を書いても構いません。

```jsx
/*
SCRIPTMETA-BEGIN
Script-ID=org.example.export-selection
Meta-Default-language=ja
Name=選択した画像を書き出す
Name[en]=Export Selected Images
Name[fr]=Exporter les images sélectionnées
Author=例示チーム
Author[en]=Example Team
Version=1.0.0
Target-App=Illustrator
Tags=image,画像,
export
Tags=image,batch
X-org.example-Group=Production Tools
X-org.example-Order=0020
X-org.example-URL=https://example.com/my%20scripts?Name=Export&Version=1.0
X-org.example-Note=
Description-BEGIN
[ja]選択した画像を書き出します。
保存先を指定してください。
\[en\] という表記を本文に表示できます。
[en]
Exports the selected images.
Choose an output folder.
[fr]
Exporte les images sélectionnées.
Choisissez un dossier de destination.
Description-END
SCRIPTMETA-END
*/
```

この例のTagsは `image`、`画像`、`export`、`batch` の順になります。`image` は重複を除きます。XタグのOrderは文字列 `0020` として返ります。

既定言語の最初のDescriptionラベルは省略でき、その前の本文は既定言語に属します。明示的なラベルを使うと、言語の境界が分かりやすくなります。

有効な `Meta-Default-language` があるローカライズされた本文では、文字としての `[`、`]`、バックスラッシュをそれぞれ `\[`、`\]`、`\\` で表せます。行頭に文字としての `[en]` を置く例は `\[en\]` です。この例のように `[ja]` 等の有効な言語ラベルを使う本文でエスケープを適用します。

有効な `Meta-Default-language` がないDescriptionは全文を通常の本文として扱います。先頭に `[en]` があっても本文の文字です。バックスラッシュもそのまま保持します。有効な言語ラベルが一つもない本文も、通常の既定本文として扱います。

同じ言語のName・Authorが複数ある場合は半角スペースで接続し、同じ言語のDescriptionが複数ある場合は空行で接続します。同じ内容も接続します。新規作成では、各言語の内容を一つにまとめて記述します。

空の追加言語を残す場合は、`Name[fr]=` のような宣言を使えます。言語の変更・解除はKitの [多言語編集API](LocalizationEditingAPI.md) を利用できます。最後の言語を解除する場合は内容を素のキーに移し、言語付きキーとMeta-Default-languageを取り除きます。

## 分類用のTags

- タグの区切りは半角カンマです。カンマ前後の空白を除き、タグ内部の空白を保持します。
- Tagsは全言語に共通です。日本語と英語の語を併記して構いません。
- Tagsが複数あれば記載順に結合し、完全に同じ文字列の重複を除きます。`image` と `Image` は別の値です。
- 空の要素は無視します。末尾のカンマは空のタグを作りません。
- 行末がカンマの場合は、続く行にもタグを記述できます。通常項目・独自項目・ブロック境界でその継続が終わります。
- `Name=Example`、`Name[en]=Example`、`client:print`、`Description-END` 等、項目やブロック境界と区別できない値は禁止です。
- Description本文内の `Tags=...` は説明の文字として扱います。

## 管理用のXタグ

形式は `X-作成者ID-項目名=値` です。作成者IDにはASCII英数字とドットを使います。小文字を標準表記とし、先頭・末尾のドット、連続するドットは禁止します。作成者ID中の記号はドットだけです。項目名にはASCII英数字とハイフンを使い、先頭・末尾を英数字にします。

例えば `X-org.example-Review-Status=Approved` と書けます。キーの照合は大文字・小文字を区別しません。

値は1行の文字列です。前後の空白を除き、内部の空白を保持します。空欄も許可します。`:`、`=`、カンマを値に使えます。同じキーの複数記述は記載順の配列になり、同じ値の重複も保持します。

引用符は値の文字として保持します。`X-org.example-Group="Production Tools"` の値には引用符も含まれます。通常は `X-org.example-Group=Production Tools` と記述すれば十分です。

URLはエンコード済みの形で記述し、スペースは `%20` とします。KitはURLを自動的に開いたり、値をデコードしたりせず、文字列として返します。

次の記述は不適です。値の途中の空白に続く既存項目記法は、旧1.xで通常項目に誤読されるため禁止します。タブ等の空白、`Name :` のような区切り前の空白、大文字・小文字の違いも判定対象です。

```text
X-org.example-Note=説明 Name=OtherName
X-org.example-Note="説明 Name=OtherName"
X-org.example-Note='説明 Name=OtherName'
```

検査対象の既存キーは `Script-ID`、`Version`、`Description`、`Target-App`、`Min-Target-Version`、`Meta-URL`、`Latest-URL`、`Latest-Version`、`Latest-Page-URL`、`Name`、`Author`、`Release-Date`、`Edit-Password-SHA256` です。

キーと値には、次の6種類の予約境界文字列を含めてはいけません。大文字・小文字を区別せず、部分一致でも禁止します。URLや引用符内にも適用します。

```text
SCRIPTMETA-BEGIN
SCRIPTMETA-END
SCRIPTMETA-DIST-BEGIN
SCRIPTMETA-DIST-END
Description-BEGIN
Description-END
```

コメント内へ保存する際には、その言語のコメント記号にも注意します。JavaScript／JSXのX値に `*/`、AppleScriptのX値に `(*` や `*)` がある場合、Kitは内容の変化を避けるため保存エラーとします。URL中の該当文字は必要に応じてパーセントエンコードします。

アプリ側の取得指定は、省略・空配列ならX値を保持せず、完全なキー名の配列なら指定分だけ、`["X-"]` なら全Xタグを取得します。ファイルに書くキーには作成者IDと項目名を含めます。編集時は取得対象外のXタグや未知項目も保全します。

詳細とAPI使用例は [独自拡張タグ仕様](CustomMetadataFields.md) を参照してください。

## AppleScriptの記述例

```applescript
(*
SCRIPTMETA-BEGIN
Script-ID=org.example.list-documents
Meta-Default-language=ja
Name=開いている書類を一覧表示
Name[en]=List Open Documents
Author=Example Team
Version=1.0.0
Tags=documents,utility
X-org.example-Group=Documents
Description-BEGIN
[ja]開いている書類の名前を一覧表示します。
[en]Lists the names of open documents.
Description-END
SCRIPTMETA-END
*)
```

## 作成・編集後の確認

Kitの解析・検証APIで、Script-ID、素のName、既定言語、言語別の内容、Tags、X値を確認します。構文に関する診断がある場合は修正し、保存時のエラーも確認します。

既存ファイルを編集した場合は、依頼対象外の内容とスクリプト本体が保全されていることを確認します。外部変更の検知とバックアップにはKitの編集セッションと条件付き保存を利用できます。

旧1.xでは素のNameが使われ、Descriptionの言語ラベルも本文に含まれます。2.1ではアプリから渡された優先言語に従い、各項目の表示値を選びます。OSの優先言語の取得とKitへの通知は利用アプリが担当します。

この文書の `jsx` と `applescript` の完全な例は、テストでKitに読み込ませ、`ja`・`en`・`fr` の指定ごとに保存前後の内容を検証しています。
