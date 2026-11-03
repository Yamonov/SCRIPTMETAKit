// TEST FIXTURE ONLY: no document or file operations.
/*
SCRIPTMETA-BEGIN
Script-ID=com.example.scriptmetakit.test.tags10
Meta-Default-language=ja
Name=[TEST 10] 説明文のエスケープとタグの記述例
Name[en]=[TEST 10] Escapes and tag examples in Description
Author=テスト用作者
Author[en]=Test author
Version=0.0.1
Target-App=Illustrator
Tags=test,fixture,escape,documentation
Description-BEGIN
[ja]
テスト専用ファイルです。次の行は言語切り替えではなく、角括弧を含む説明文です。
\[en\] この行も日本語の説明として残ります。
パスの記述例: C:\\Test\\fixtures
Tags=example-only,not-a-real-tag
上の Tags 行は説明文の一部であり、実際のタグ一覧には加えません。
[en]
Test fixture for escaped brackets, backslashes and a Tags example inside Description.
\[ja\] This line stays in the English description.
Example path: C:\\Test\\fixtures
Tags=example-only,not-a-real-tag
Description-END
SCRIPTMETA-END
*/
(function () {
    return "SCRIPTMETA_TEST_10";
}());
