$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $IsWindows -or $env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    throw "Windows packaging requires a native Windows x64 host"
}
$dirty = @(& git -C $Root status --porcelain --untracked-files=all)
if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) {
    throw "Windows release packaging requires a clean Git worktree"
}

$Version = (Get-Content (Join-Path $Root "apps/desktop/package.json") -Raw | ConvertFrom-Json).version
if ($Version -ne "1.2.1") {
    throw "Windows release version must be 1.2.1"
}
$Thumbprint = ($env:WINDOWS_CERTIFICATE_THUMBPRINT -replace '\s', '').ToUpperInvariant()
if ($Thumbprint -notmatch '^[0-9A-F]{40}$') {
    throw "WINDOWS_CERTIFICATE_THUMBPRINT must be exactly 40 hexadecimal characters"
}
$Certificate = Get-Item "Cert:\CurrentUser\My\$Thumbprint" -ErrorAction Stop
if (-not $Certificate.HasPrivateKey) {
    throw "Configured Authenticode certificate has no private key"
}
$CodeSigningOid = "1.3.6.1.5.5.7.3.3"
if (-not ($Certificate.EnhancedKeyUsageList.ObjectId.Value -contains $CodeSigningOid)) {
    throw "Configured certificate is not valid for code signing"
}

$SigningConfig = @{
    bundle = @{
        windows = @{
            certificateThumbprint = $Thumbprint
            digestAlgorithm = "sha256"
            timestampUrl = "http://timestamp.digicert.com"
        }
    }
} | ConvertTo-Json -Depth 5 -Compress

$PreviousConfig = $env:TAURI_CONFIG
try {
    $env:TAURI_CONFIG = $SigningConfig
    Push-Location $Root
    try {
        & npm exec --workspace "@mdviewer/desktop" tauri -- build --ci --bundles nsis --config src-tauri/tauri.windows.conf.json
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri Windows packaging failed"
        }
    } finally {
        Pop-Location
    }
} finally {
    $env:TAURI_CONFIG = $PreviousConfig
}

$Candidates = @(Get-ChildItem (Join-Path $Root "target/release/bundle/nsis") -Filter "*-setup.exe" -File)
if ($Candidates.Count -ne 1) {
    throw "Expected exactly one NSIS setup executable"
}
$BuiltExecutable = Join-Path $Root "target/release/mdviewer-desktop.exe"
foreach ($File in @($BuiltExecutable, $Candidates[0].FullName)) {
    $Signature = Get-AuthenticodeSignature $File
    if ($Signature.Status -ne "Valid" -or $Signature.SignerCertificate.Thumbprint -ne $Thumbprint) {
        throw "Authenticode verification failed for $File"
    }
}

$Dist = Join-Path $Root "dist/windows-x64"
New-Item -ItemType Directory -Force $Dist | Out-Null
$Installer = Join-Path $Dist "MDViewer-$Version-x64-setup.exe"
Copy-Item $Candidates[0].FullName $Installer -Force
$Commit = (& git -C $Root rev-parse HEAD).Trim()
$Receipt = [ordered]@{
    schemaVersion = 1
    platform = "windows"
    version = $Version
    target = "x86_64-pc-windows-msvc"
    publishable = $false
    signed = $true
    provenance = "pending_github_attestation"
    commit = $Commit
    certificateThumbprint = $Thumbprint
    artifacts = [ordered]@{
        installer = [ordered]@{
            name = Split-Path $Installer -Leaf
            sha256 = (Get-FileHash $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
}
$Receipt | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Dist "package-receipt-windows-x64.json") -Encoding utf8NoBOM
Write-Host "WINDOWS PACKAGE COMPLETE: production verification is still required."
