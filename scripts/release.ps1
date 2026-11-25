# Builds dist\BosunSystemCare-Setup-<version>.exe: the app and tray in
# release mode, embedded into the bosun-setup installer, plus a SHA-256 file.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

# its own target folder, so a copy running from target\release doesn't block it
$target = Join-Path $root 'target\dist'
$env:CARGO_TARGET_DIR = $target
$env:BOSUN_PAYLOAD_DIR = Join-Path $target 'release'
try {
    cargo build --release -p bosun-app -p bosun-tray
    if ($LASTEXITCODE) { throw 'app build failed' }
    # build.rs re-embeds when the payload files change
    cargo build --release -p bosun-setup
    if ($LASTEXITCODE) { throw 'setup build failed' }
} finally {
    Remove-Item Env:BOSUN_PAYLOAD_DIR, Env:CARGO_TARGET_DIR
}

New-Item -ItemType Directory -Force dist | Out-Null
$out = "dist\BosunSystemCare-Setup-$version.exe"
Copy-Item (Join-Path $target 'release\bosun-setup.exe') $out -Force
$hash = (Get-FileHash $out -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $out -Leaf)" | Out-File "$out.sha256" -Encoding ascii
"$out  $([math]::Round((Get-Item $out).Length / 1MB, 2)) MB  sha256 $hash"
