// TEST FIXTURE ONLY: no document or file operations.
/*
SCRIPTMETA-BEGIN
Script-ID=com.example.scriptmetakit.test.tags04
Meta-Default-language=ja
Name=[TEST 04] 複数の Tags と重複
Name[en]=[TEST 04] Repeated Tags fields
Author=テスト用作者
Author[en]=Test author
Version=0.0.1
Target-App=Illustrator
Tags=test,fixture,画像,image
Tags=image,print,test
Tags=Image,画像,export
Tags=
Description-BEGIN
[ja]
テスト専用ファイルです。複数の Tags を出現順で結合します。
完全に同じタグだけを取り除くため、image と Image は別々に残ります。
最後の空の Tags は、それまでのタグを消しません。
[en]
Test fixture for merging Tags fields and removing exact duplicates.
The tags image and Image remain distinct. An empty Tags field clears nothing.
Description-END
SCRIPTMETA-END
*/
(function () {
    return "SCRIPTMETA_TEST_04";
}());
