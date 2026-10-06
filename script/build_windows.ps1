param([switch]$Release)

# Cargo writes progress to stderr; use its exit code to detect native failures.
$ErrorActionPreference = "Continue"
$KitRoot = Split-Path -Parent $PSScriptRoot
$CargoArguments = @(
    "build", "--locked",
    "--manifest-path", (Join-Path $KitRoot "Cargo.toml"),
    "-p", "scriptmetakit_ffi",
    "--features", "blocking-http,native-watch",
    "--target", "x86_64-pc-windows-msvc"
)
if ($Release) { $CargoArguments += "--release" }
$PreviousEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$BuildRustFlags = @()
if ($PreviousEncodedRustFlags) {
    $BuildRustFlags = $PreviousEncodedRustFlags -split [char]31
} elseif ($env:RUSTFLAGS) {
    $BuildRustFlags = $env:RUSTFLAGS -split '\s+'
}
$BuildRustFlags += "--remap-path-prefix=$KitRoot=."
$BuildRustFlags += "--remap-path-prefix=$env:USERPROFILE=~"
$env:CARGO_ENCODED_RUSTFLAGS = $BuildRustFlags -join [char]31
try {
    & cargo @CargoArguments
    $BuildExitCode = $LASTEXITCODE
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $PreviousEncodedRustFlags
}
if ($BuildExitCode -ne 0) { exit $BuildExitCode }

$BuildProfile = if ($Release) { "release" } else { "debug" }
Write-Output (Join-Path $KitRoot "target/x86_64-pc-windows-msvc/$BuildProfile/scriptmetakit_ffi.dll")
