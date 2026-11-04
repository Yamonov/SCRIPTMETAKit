# SCRIPTMETAKit 2.1.1 Swift／C接続の修正と検証

## 修正した問題

2.1.0のSwift APIでメタデータを保存すると、x86_64では通常保存と外部変更検知の呼び出しがSIGSEGVで終了しました。Scriptaを含まないKitだけのSwift CLIでも再現しました。同じプログラムのarm64実行は成功していました。

保存関数 `smk_engine_write_script_metadata_file_if_unchanged_v3` は、ポインター3個、ポインターと長さからなる構造体2個、結果のポインターを受け取ります。Swift側の `@_silgen_name` 宣言はSwiftの呼び出し規約を使うため、x86_64のC ABIとは後半の引数の渡し方が一致していませんでした。

Swift公式の [Underscored Attributes](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/UnderscoredAttributes.md#_silgen_nameraw-cname) は、C関数にはCヘッダーまたはCモジュールのimportを使うよう説明しています。

## 実装

- Kit内のSwiftコードは `ScriptMetaKitFFI` をimportし、C関数とC構造体を直接使用します。
- C関数の手書き宣言とC構造体を写したSwift構造体を削除しました。保存以外の走査、監視、設定、編集、パス判定、コールバックも同じ方針で修正しました。
- Cのステータスを、既存の公開Swiftエラーが持つ `Int32` へ内部で変換します。
- 公開Swift API、Rust／Cの関数と構造体のABI、メタデータ形式、キャッシュのschema／projectionは維持します。
- Swift 6.1以降、Swift 5／Swift 6言語モード、macOS 13以降という要件を維持します。

2.1.0と同じネイティブライブラリに修正後のSwiftコードを接続して、両CPUで通常保存と外部変更検知が成功することを確認しました。ネイティブ側の呼び出し規約を変える対応は行っていません。

## Swift公開APIの回帰テスト

`tests/SwiftAPIRegression` は、アプリやTestingを含まない独立したSwiftパッケージです。`script/validate_swift_api.py` から次の8通りを実行します。

| 言語モード | CPU | ビルド |
| --- | --- | --- |
| Swift 5 | arm64／x86_64 | Debug／Release |
| Swift 6 | arm64／x86_64 | Debug／Release |

各実行は次の動作を検証します。

1. 多言語・Tags・独自項目を含むスクリプトの走査と、型付き結果の取得。
2. 編集セッションからの通常保存。
3. バックアップ付き保存と、元の内容・バックアップ履歴の取得。
4. 外部変更の検出と、外部変更後の内容を上書きしないこと。
5. シンボリックリンク経由の保存と、リンクの保持。
6. 日本語・英語・フランス語、空の言語別値、Tags、重複した独自値、空値、URL、未知フィールド、スクリプト本文の保全。

回帰テストはGitHub Actionsにも追加しました。Rust／CだけのテストではSwiftの呼び出し規約の不一致を検出できないため、両CPUからSwift公開APIを実行する検証を独立して行います。

旧版SwiftPMのUniversal CLIビルドでは、バイナリ依存のdylibが実行ファイルの検索先へ配置されない場合があります。検証スクリプトは、解決されたKit依存の成果物をmanifestのハッシュと照合し、テスト実行ファイルの隣へ配置します。テスト専用の `@loader_path` によりそのdylibを読み込み、実行結果にも使用したハッシュを記録します。公開SDKや利用アプリの読み込み方法を変更するものではありません。

```sh
python3 script/validate_swift_api.py
```

XcodeBuildMCP CLIを使用する環境では、`--builder xcodebuildmcp` を指定できます。x86_64の実行には、Intel MacまたはApple Silicon上のRosettaが必要です。このリリースの手元でのx86_64実行確認にはRosettaを使用します。

## 手元での検証結果

- Swift公開APIの8通りの組み合わせは、すべて成功しました。
- Swift 5とSwift 6の既存テストは各114件が成功しました。
- macOSのRust／Cテスト418件、Clippy、既定機能なしのコンパイル、ライセンス確認が成功しました。
- Universalバイナリの両CPUで、公開C関数145個、ネイティブリンク、版と機能の取得、エンジンの生成・解放を確認しました。

## リリース時の確認

macOS UniversalとWindows x64は同じ2.1.1のソースから作成します。バイナリの版、全145関数の公開、Swiftの全テスト、両OSのRust／Cテスト、macOS署名・公証を確認します。公開後には配布物を再取得し、ハッシュと公証記録を照合したうえで、公開タグを指定したSwiftパッケージでも回帰テストを実行します。

既存タグ2.1.0とその配布物は置き換えません。Scriptaなどの利用側は、SwiftPMの依存バージョンを2.1.1以降へ更新します。
