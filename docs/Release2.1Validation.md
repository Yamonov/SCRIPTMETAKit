# SCRIPTMETAKit 2.1.0 レビューと検証

Rust共通処理、C ABI、macOS固有I/O、Windows固有処理、Swift API、編集・保存、キャッシュ、更新確認、監視の各層を対象に確認しました。

## レビューで修正した内容

- Swiftで手動作成したXタグ辞書も、キーの大文字・小文字を区別せず取得・更新できるようにしました。更新時は同じキーの別表記を置き換えます。
- コメントの終了記号を含むX値は、保存先の書式に応じてエラーとします。コメント用の表記調整でX値が変わることを防ぎます。
- シンボリックリンク経由のテキストとコンパイル済みAppleScriptの保存は、リンクを保って実体を更新します。バックアップの参照と復元にも対応します。
- 名前付きパイプ等の非通常ファイルは、同期的な編集用読み取り・保存の前に拒否します。書き手の接続待ちでAPIが停止することを防ぎます。
- Windowsのファイル置き換えでは、DACLと継承の保護設定を新しいファイルへコピーします。ACLを保持するファイルシステムかを確認し、対応するボリュームで適用します。

WindowsのACLについては、公開版2.0.1のDLLをWindows実機から呼び出し、保護された個別ACLが保存後に継承状態へ変わることを一時ファイルで再現しました。2.1.0の回帰テストは継承あり・なしを確認します。Windows APIの取り扱いは [GetNamedSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getnamedsecurityinfow)、[SetNamedSecurityInfo](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow)、[ボリュームのACL対応フラグ](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationbyhandlew) に従います。

## 検証する範囲

- macOSのarm64／x86_64とWindows x64のReleaseビルド。
- 両OSでのRust全機能テスト、Clippy、既定機能なしのコンパイル。
- Swift 5／Swift 6でのパッケージテスト。
- 両macOSスライスの公開C関数、ネイティブリンク・実行、Mach-O文字列テーブルの整合性。
- Xcodeアプリへのパッケージ組み込みとヘッドレスの動作確認。
- Windows DLLの動的ロード、公開関数、Xタグと多言語編集APIの呼び出し。
- ソースと成果物のハッシュ、macOS署名・公証、公開後の再ダウンロード。

## AI向け記述ガイド

[メタデータ記述ガイド](MetadataAuthoring.md) を記述時の入口にしました。通常のTags、Name・Author・Descriptionの多言語記述、Xタグ、予約文字列、コメント書式、空値、重複の取り扱いを一つの文書で確認できます。

JavaScript／JSXとAppleScriptの完全な3例をKitで解析し、日本語・英語・フランス語の各指定で保存前後の意味が同じになることをテストします。

## 2.1.0の検証結果

| 対象 | 結果 |
| --- | --- |
| macOSのRust／C全機能テスト | 418件成功 |
| Windows実機のRust／C全機能テスト | 390件成功 |
| Swift 5 | 114件成功 |
| Swift 6 | 114件成功 |
| 両OSのClippyと既定機能なしのコンパイル | 成功 |
| 両macOSスライスとWindows DLLの公開C関数 | 145個すべて確認 |
| Xcode Releaseアプリへの組み込み・実行 | arm64／x86_64で成功 |
| Windows DLLの動的ロードと管理コードからの呼び出し | 成功 |
| WindowsのDACL保全とv3保存 | 個別DACLと保護設定、X値の更新を確認 |
| 記述ガイドの完全な例 | 3例を3言語で読み書きして確認 |
| 既存メタデータの通常読み取り比較 | 471件で公開版2.0.1との差分0件 |

macOSはApple Silicon上で検証し、x86_64の実行にはRosettaを使用しました。WindowsはNucBox G10のWindows実機で、Rust 1.94.0とMSVCを使用しました。

1,000件の多言語スクリプトを使うXcodeアプリの検証では、初回読み取りと保存は約0.38秒、キャッシュ復元は約0.08秒、言語変更後の再読み込みは約0.40秒でした。診断エラーは0件です。測定値はこの検証環境での結果です。

既存データの比較対象は、Iwashiya_Scriptsから取得した70件と旧jsxフォルダから取得した401件のメタデータです。優先言語を `ja`、`en`、`fr` の同じ順で渡し、通常項目・Tags・選択言語を比較しました。更新チェックとコンパイル済みOSAの逆コンパイルを無効にした読み取りです。

成果物の署名・公証、ハッシュ、公開後の取得確認はリリース処理で検証します。
