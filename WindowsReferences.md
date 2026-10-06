# Windows references (1.3.3)

Windows Shell links (`.lnk`) are filesystem references in the scanner and native
watcher. Shell-link resolution is provided internally on Windows without adding
a field to the published `ScannerOptions` structure. The existing
`resolve_registered_path` API and `smk_resolve_registered_path` C ABI expose the
same resolution result used by file-list and metadata scans.

- Registered roots may be real folders or links to folders.
- A real or referenced folder may contain real files, file links, real folders,
  and folder links. Folder links are traversed using the existing scanner limits.
- `display_path` retains the logical location under the registered root;
  `source_path` identifies the reference entry and `resolved_path` identifies the
  current target. Applications should keep root/tab identity when editing a
  shortcut and authorize execution through the current Kit-visible tree.
- Broken, denied, unsupported, and cyclic references retain their resolution
  status. They must not authorize script execution.
- Native watches cover reference sources and resolved file/folder targets,
  including references outside the registered folder. Retargeting and target
  recovery request rescanning. Missing physical watches retry independently so
  they do not prevent available roots from being monitored.

Shell-link resolution reads the stored filesystem target through `IShellLinkW`.
It does not launch the link, use its command-line arguments, invoke Shell search
or repair, or rewrite the link. Original content is not copied. Relative/non-file
Shell targets and links larger than 1 MiB are unsupported; link traversal is
limited to 32 hops with cycle detection. Renaming a target to another location
requires updating the reference; disappearance and return at the same target
path are supported.

Windows regression coverage: `tests/windows_shortcuts.rs` (9 tests), alongside
workspace scanner, watcher and FFI suites. Runtime verification was on Windows
x64 with local temporary folders. Physical drive removal and network-share
outages were not exercised by these tests.
