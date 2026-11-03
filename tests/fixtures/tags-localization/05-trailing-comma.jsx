// TEST FIXTURE ONLY: no document or file operations.
/*
SCRIPTMETA-BEGIN
Script-ID=com.example.scriptmetakit.test.tags05
Meta-Default-language=ja
Name=[TEST 05] 末尾カンマと空要素
Name[en]=[TEST 05] Trailing comma and empty entries
Author=テスト用作者
Author[en]=Test author
Version=0.0.1
Target-App=Illustrator
Tags=test,fixture,,layout,print,
Description-BEGIN
[ja]
テスト専用ファイルです。連続したカンマと最後のカンマから、空のタグを作りません。
説明ブロックの開始位置でタグの読み取りを終了します。
[en]
Test fixture for empty tag entries and a trailing comma before Description.
Description-END
SCRIPTMETA-END
*/
(function () {
    return "SCRIPTMETA_TEST_05";
}());
