// TEST FIXTURE ONLY: no document or file operations.
/*
SCRIPTMETA-BEGIN
Script-ID=com.example.scriptmetakit.test.tags02
Meta-Default-language=ja
Name=[TEST 02] タグの前後と内部の空白
Name[en]=[TEST 02] Spaces around and inside tags
Author=テスト用作者
Author[en]=Test author
Version=0.0.1
Target-App=Illustrator
Tags= test , fixture , 画像 , image , batch export
Description-BEGIN
テスト専用ファイルです。カンマ前後の空白を取り除きます。
「batch export」の内部の半角スペースは保持します。
この説明には言語ラベルがないため、既定言語の日本語として扱います。
[en]
this is the test.
Description-END
SCRIPTMETA-END
*/
(function () {
    return "SCRIPTMETA_TEST_02";
}());
