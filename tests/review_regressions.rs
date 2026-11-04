use scriptmetakit::{
    MetadataEditExtensions, RootRegistration, ScanMode, ScanRequest, ScriptMetaCommentStyle,
    ScriptMetaEditState, ScriptMetaKitConfig, ScriptMetaKitEngine, ScriptMetaWriteMode,
    ScriptMetadataDraft, render_script_metadata_block, render_script_metadata_for_style,
    write_script_metadata_to_text,
};
use std::{fs, path::Path};

fn draft() -> ScriptMetadataDraft {
    ScriptMetadataDraft {
        script_id: "com.example.review".into(),
        name: Some("Name".into()),
        description: Some("Body".into()),
        ..Default::default()
    }
}

#[cfg(unix)]
#[test]
fn named_pipe_metadata_operations_fail_without_opening_the_pipe() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    const CHILD_PATH: &str = "SMK_NAMED_PIPE_METADATA_TEST";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let path = std::path::PathBuf::from(path);
        let inspection = scriptmetakit::inspect_script_file(&path);
        assert!(!inspection.can_edit_scriptmeta && !inspection.can_append_scriptmeta);
        assert!(scriptmetakit::read_script_metadata_draft_from_file(&path).is_err());
        assert!(scriptmetakit::read_script_metadata_edit_preview_from_file(&path, 128).is_err());
        assert!(
            scriptmetakit::write_script_metadata_to_file(
                &path,
                &draft(),
                ScriptMetaWriteMode::InsertOrReplace,
                None
            )
            .is_err()
        );
        println!("NAMED_PIPE_METADATA_GUARD_OK");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pipe.jsx");
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "named_pipe_metadata_operations_fail_without_opening_the_pipe",
            "--nocapture",
        ])
        .env(CHILD_PATH, &path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("metadata operations blocked on a named pipe");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("NAMED_PIPE_METADATA_GUARD_OK"));
}

#[cfg(unix)]
#[test]
fn metadata_edits_and_backup_restore_preserve_symbolic_links() {
    use scriptmetakit::{
        ScriptMetaBackupOptions, read_script_metadata_draft_from_file, restore_scriptmeta_backup,
        scriptmeta_backup_generations, write_script_metadata_to_file_if_unchanged,
    };
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.jsx");
    let link = directory.path().join("link.jsx");
    let source =
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=review.symlink\nName=Original\nSCRIPTMETA-END\n*/\n";
    fs::write(&target, source).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let read = read_script_metadata_draft_from_file(&link).unwrap();
    let mut metadata = read.draft;
    metadata.name = Some("Updated".into());
    let backups = ScriptMetaBackupOptions {
        root_directory: directory.path().join("backups"),
    };
    write_script_metadata_to_file_if_unchanged(
        &link,
        &metadata,
        ScriptMetaWriteMode::ReplaceOnly,
        Some(&backups),
        Some(&read.source_fingerprint),
    )
    .unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(&target)
            .unwrap()
            .contains("Name=Updated")
    );
    let generations = scriptmeta_backup_generations(&link, &backups)
        .unwrap()
        .into_iter()
        .filter(|generation| !generation.is_current_file)
        .collect::<Vec<_>>();
    assert_eq!(generations.len(), 1);
    restore_scriptmeta_backup(&link, &backups, &generations[0].id).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), source);
}

#[cfg(target_os = "macos")]
#[test]
fn compiled_applescript_metadata_edits_preserve_symbolic_links() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.applescript");
    let target = directory.path().join("target.scpt");
    let link = directory.path().join("link.scpt");
    fs::write(&source, "(*\nSCRIPTMETA-BEGIN\nScript-ID=review.compiled-link\nName=Original\nSCRIPTMETA-END\n*)\nreturn 1\n").unwrap();
    assert!(
        std::process::Command::new("/usr/bin/osacompile")
            .arg("-o")
            .arg(&target)
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let read = scriptmetakit::read_script_metadata_draft_from_file(&link).unwrap();
    let mut metadata = read.draft;
    metadata.name = Some("Updated".into());
    scriptmetakit::write_script_metadata_to_file_if_unchanged(
        &link,
        &metadata,
        ScriptMetaWriteMode::ReplaceOnly,
        None,
        Some(&read.source_fingerprint),
    )
    .unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let after = scriptmetakit::read_script_metadata_draft_from_file(&target).unwrap();
    assert_eq!(after.draft.name.as_deref(), Some("Updated"));
}

#[test]
fn render_and_write_protect_comment_delimiters_consistently() {
    let mut metadata = draft();
    metadata.description = Some("*/ closes JS; (* starts AppleScript; *) closes it".into());
    for (style, name, opening, closing) in [
        (
            ScriptMetaCommentStyle::JavaScriptBlock,
            "sample.jsx",
            "/*",
            "*/",
        ),
        (
            ScriptMetaCommentStyle::AppleScriptBlock,
            "sample.applescript",
            "(*",
            "*)",
        ),
    ] {
        let rendered = render_script_metadata_for_style(&metadata, style).unwrap();
        assert_eq!(rendered.matches(closing).count(), 1, "{rendered}");
        assert_eq!(rendered.matches(opening).count(), 1, "{rendered}");
        let written = write_script_metadata_to_text(
            "",
            Path::new(name),
            &metadata,
            ScriptMetaWriteMode::InsertOnly,
        )
        .unwrap();
        assert_eq!(written.text.matches(closing).count(), 1);
        assert_eq!(written.text.matches(opening).count(), 1);
    }
    let plain =
        render_script_metadata_for_style(&metadata, ScriptMetaCommentStyle::PlainText).unwrap();
    assert!(plain.contains(metadata.description.as_deref().unwrap()));
}

#[cfg(windows)]
#[test]
fn windows_metadata_saves_preserve_custom_dacl_and_inheritance() {
    fn acl(path: &Path, setup: bool, protected: bool) -> String {
        let script = if setup {
            r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$kitPath = $env:SMK_ACL_TEST_PATH
$kitAcl = Get-Acl -LiteralPath $kitPath
$kitAcl.SetAccessRuleProtection([bool]::Parse($env:SMK_ACL_TEST_PROTECTED), $true)
$kitSid = New-Object Security.Principal.SecurityIdentifier('S-1-5-32-545')
$kitRule = New-Object Security.AccessControl.FileSystemAccessRule(
    $kitSid,
    [Security.AccessControl.FileSystemRights]::Read,
    [Security.AccessControl.AccessControlType]::Allow
)
$kitAcl.AddAccessRule($kitRule)
Set-Acl -LiteralPath $kitPath -AclObject $kitAcl
(Get-Acl -LiteralPath $kitPath).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)
"#
        } else {
            r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
(Get-Acl -LiteralPath $env:SMK_ACL_TEST_PATH).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)
"#
        };
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .env("SMK_ACL_TEST_PATH", path)
            .env("SMK_ACL_TEST_PROTECTED", protected.to_string())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
    let directory = tempfile::tempdir().unwrap();
    for protected in [false, true] {
        let path = directory.path().join(format!("acl-{protected}.jsx"));
        fs::write(
            &path,
            "/*\nSCRIPTMETA-BEGIN\nScript-ID=windows.acl\nName=Original\nSCRIPTMETA-END\n*/\n",
        )
        .unwrap();
        let before = acl(&path, true, protected);
        let metadata = ScriptMetadataDraft {
            script_id: "windows.acl".into(),
            name: Some("Updated".into()),
            ..Default::default()
        };
        scriptmetakit::write_script_metadata_to_file(
            &path,
            &metadata,
            ScriptMetaWriteMode::ReplaceOnly,
            None,
        )
        .unwrap();
        assert_eq!(acl(&path, false, protected), before);
        assert!(fs::read_to_string(&path).unwrap().contains("Name=Updated"));
    }
}

#[test]
fn scalar_field_line_breaks_cannot_create_new_metadata_fields() {
    for newline in ["\n", "\r", "\r\n"] {
        for field in ["Script-ID", "Name", "Author", "Target-App", "Release-Date"] {
            let mut metadata = draft();
            let injected = format!("value{newline}Version=9.9");
            match field {
                "Script-ID" => metadata.script_id = injected,
                "Name" => metadata.name = Some(injected),
                "Author" => metadata.author = Some(injected),
                "Target-App" => metadata.target_app = Some(injected),
                _ => metadata.release_date = Some(injected),
            }
            assert!(
                render_script_metadata_block(&metadata).is_err(),
                "{field} accepted a line break"
            );
        }
    }
}

#[test]
fn inline_legacy_fields_cannot_silently_change_other_values_on_write() {
    let mut metadata = draft();
    metadata.name = Some("Example Version=9.9".into());
    assert!(render_script_metadata_block(&metadata).is_err());
    metadata.extensions = Some(MetadataEditExtensions {
        default_language: Some("en".into()),
        ..Default::default()
    });
    let rendered = render_script_metadata_block(&metadata).unwrap();
    let parsed = scriptmetakit::parse_script_metadata(&rendered).unwrap();
    assert_eq!(parsed.name, metadata.name);
    assert!(parsed.version.is_none());
    metadata.target_app = Some("Example Name=other".into());
    assert!(render_script_metadata_block(&metadata).is_err());
    let distribution = scriptmetakit::DistributionMetadataDraft {
        script_id: "one Script-ID=two".into(),
        version: Some("1.0".into()),
        latest_url: None,
        latest_page_url: None,
    };
    assert!(scriptmetakit::render_distribution_metadata_block(&[distribution]).is_err());
}

#[test]
fn description_end_markers_are_rejected_for_every_newline_style() {
    for newline in ["\n", "\r", "\r\n"] {
        for localized in [false, true] {
            let mut metadata = draft();
            metadata.description = Some(format!("one{newline}Description-END{newline}two"));
            if localized {
                metadata.extensions = Some(MetadataEditExtensions {
                    default_language: Some("ja".into()),
                    ..Default::default()
                });
            }
            assert!(
                render_script_metadata_block(&metadata).is_err(),
                "accepted {newline:?}, localized={localized}"
            );
        }
    }
}

#[test]
fn unrelated_edits_preserve_description_indentation_and_trailing_spaces() {
    for newline in ["\n", "\r", "\r\n"] {
        for localized in [false, true] {
            let extra = if localized {
                "Meta-Default-language=ja\nName[en]=English\n"
            } else {
                ""
            };
            for body in [
                "    indented code  ",
                "    first\nsecond  ",
                "\n    first\n[en]\n    English  \n",
            ] {
                let source = format!("/*\nSCRIPTMETA-BEGIN\nScript-ID=whitespace\nName=Original\n{extra}Description-BEGIN\n{body}\nDescription-END\nSCRIPTMETA-END\n*/\n").replace('\n', newline);
                let path = Path::new("sample.jsx");
                let before =
                    scriptmetakit::read_script_metadata_draft_from_text(&source, path).unwrap();
                let mut edited = before.draft.clone();
                edited.name = Some("Changed".into());
                let written = write_script_metadata_to_text(
                    &source,
                    path,
                    &edited,
                    ScriptMetaWriteMode::ReplaceOnly,
                )
                .unwrap();
                let after =
                    scriptmetakit::read_script_metadata_draft_from_text(&written.text, path)
                        .unwrap();
                assert_eq!(
                    after.draft.description, before.draft.description,
                    "{newline:?}, localized={localized}, body={body:?}"
                );
            }
        }
    }
}

#[test]
fn outer_end_marker_cannot_truncate_legacy_or_localized_text() {
    for field in [
        "Script-ID",
        "Name",
        "Author",
        "Target-App",
        "Release-Date",
        "Description",
    ] {
        let mut metadata = draft();
        let injected = "literal scriptmeta-end example".to_string();
        match field {
            "Script-ID" => metadata.script_id = injected,
            "Name" => metadata.name = Some(injected),
            "Author" => metadata.author = Some(injected),
            "Target-App" => metadata.target_app = Some(injected),
            "Release-Date" => metadata.release_date = Some(injected),
            _ => metadata.description = Some(injected),
        }
        assert!(
            render_script_metadata_block(&metadata).is_err(),
            "{field} accepted the outer end marker"
        );
    }
}

#[test]
fn unread_compiled_scripts_do_not_become_editable_from_cache() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("unread.scpt"),
        b"not decompiled by this test",
    )
    .unwrap();
    let mut settings = ScriptMetaKitConfig::default();
    settings.scanner.decompile_compiled_osa_during_scan = false;
    settings.update_check.enabled = false;
    settings.watcher.enabled = false;
    let mut engine = ScriptMetaKitEngine::new(settings).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "root",
            directory.path(),
        )])
        .unwrap();
    for _ in 0..3 {
        let result = engine
            .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
            .unwrap();
        let record = &result
            .catalog_snapshot
            .as_ref()
            .unwrap()
            .candidate_cache
            .records[0];
        assert_eq!(record.scriptmeta_edit_state, ScriptMetaEditState::Unknown);
        assert!(!record.can_edit_scriptmeta && !record.can_append_scriptmeta);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn rendered_applescript_comments_compile_without_executing_the_script() {
    let directory = tempfile::tempdir().unwrap();
    let mut metadata = draft();
    for body in ["Literal (*", "Literal *)", "Literal (* and *)"] {
        metadata.description = Some(body.into());
        let rendered =
            render_script_metadata_for_style(&metadata, ScriptMetaCommentStyle::AppleScriptBlock)
                .unwrap();
        let written = write_script_metadata_to_text(
            "",
            Path::new("synthetic.applescript"),
            &metadata,
            ScriptMetaWriteMode::InsertOnly,
        )
        .unwrap()
        .text;
        for text in [rendered, written] {
            let source = format!("{text}\nreturn 1\n");
            let output = std::process::Command::new("/usr/bin/osacompile")
                .arg("-o")
                .arg(directory.path().join("synthetic.scpt"))
                .arg("-e")
                .arg(source)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{body}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn distribution_writes_cannot_add_records_or_terminate_the_block_from_a_value() {
    for script_id in [
        "one\nScript-ID=two",
        "one\rScript-ID=two",
        "SCRIPTMETA-DIST-END",
    ] {
        let record = scriptmetakit::DistributionMetadataDraft {
            script_id: script_id.into(),
            version: Some("1.0".into()),
            latest_url: None,
            latest_page_url: None,
        };
        assert!(scriptmetakit::render_distribution_metadata_block(&[record]).is_err());
    }
}

#[test]
fn failed_metadata_reads_are_retried_even_when_file_identity_is_unchanged() {
    use scriptmetakit::scanner::{
        ExtensionPolicy, PathResolutionStatus, ScannerOptions, scan_metadata_roots,
    };
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("sample.jsx"),
        "/*\nSCRIPTMETA-BEGIN\nScript-ID=recovered\nName=Recovered\nSCRIPTMETA-END\n*/",
    )
    .unwrap();
    let registration = RootRegistration::file_list_and_metadata("root", directory.path());
    let options = ScannerOptions::default();
    let extensions = ExtensionPolicy::default();
    let first = scan_metadata_roots([&registration], &options, &extensions, None);
    assert_eq!(first.all_items.len(), 1);
    for status in [
        PathResolutionStatus::PermissionDenied,
        PathResolutionStatus::Broken,
        PathResolutionStatus::Unsupported,
    ] {
        // Model a previous read failure after stat succeeded, without changing
        // permissions or racing real filesystem operations.
        let mut failed = first.candidate_cache.clone();
        let record = &mut failed.records[0];
        record.item = None;
        record.has_scriptmeta = false;
        record.can_edit_scriptmeta = false;
        record.scriptmeta_edit_state = ScriptMetaEditState::Unknown;
        record.resolution_status = status;
        record.resolution_message = Some("previous read failure".into());
        let recovered = scan_metadata_roots([&registration], &options, &extensions, Some(&failed));
        assert_eq!(
            recovered.all_items.len(),
            1,
            "failed to recover from {status:?}"
        );
        assert!(
            recovered.candidate_cache.records[0]
                .resolution_message
                .is_none()
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn unavailable_compiled_source_is_never_reported_as_appendable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("run-only.scpt");
    let output = std::process::Command::new("/usr/bin/osacompile")
        .arg("-x")
        .arg("-o")
        .arg(&path)
        .arg("-e")
        .arg("return 1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut settings = ScriptMetaKitConfig::default();
    settings.scanner.decompile_compiled_osa_during_scan = true;
    settings.watcher.enabled = false;
    let mut engine = ScriptMetaKitEngine::new(settings).unwrap();
    engine
        .set_roots(vec![RootRegistration::file_list_and_metadata(
            "root",
            directory.path(),
        )])
        .unwrap();
    for _ in 0..2 {
        let result = engine
            .scan_roots(ScanRequest::all(ScanMode::FileListAndMetadata))
            .unwrap();
        let record = &result
            .catalog_snapshot
            .as_ref()
            .unwrap()
            .candidate_cache
            .records[0];
        assert!(record.resolution_message.is_some());
        assert!(!record.can_edit_scriptmeta && !record.can_append_scriptmeta);
        assert_eq!(record.scriptmeta_edit_state, ScriptMetaEditState::Unknown);
    }
}
