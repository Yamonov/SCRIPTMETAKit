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

公開前の検証結果をこの文書へ追記します。
