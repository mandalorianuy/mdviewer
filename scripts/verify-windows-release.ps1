$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $IsWindows -or $env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    throw "Windows verification requires a native Windows x64 host"
}
$dirty = @(& git -C $Root status --porcelain --untracked-files=all)
if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) {
    throw "Windows release verification requires a clean Git worktree"
}

$Dist = Join-Path $Root "dist/windows-x64"
$ReceiptPath = Join-Path $Dist "package-receipt.json"
$Receipt = Get-Content $ReceiptPath -Raw | ConvertFrom-Json
if ($Receipt.schemaVersion -ne 1 -or $Receipt.platform -ne "windows" -or $Receipt.version -ne "1.2.1") {
    throw "Invalid Windows package receipt"
}
if ($Receipt.target -ne "x86_64-pc-windows-msvc" -or $Receipt.publishable -or -not $Receipt.signed) {
    throw "Invalid Windows receipt state"
}
if ($Receipt.commit -ne (& git -C $Root rev-parse HEAD).Trim()) {
    throw "Windows receipt does not match Git HEAD"
}
$Installer = Join-Path $Dist $Receipt.artifacts.installer.name
if ((Get-FileHash $Installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Receipt.artifacts.installer.sha256) {
    throw "Windows installer checksum mismatch"
}

function Assert-Authenticode([string]$Path) {
    $Signature = Get-AuthenticodeSignature $Path
    if ($Signature.Status -ne "Valid") {
        throw "Authenticode signature is not valid for $Path"
    }
    if ($Signature.SignerCertificate.Thumbprint -ne $Receipt.certificateThumbprint) {
        throw "Unexpected Authenticode signer for $Path"
    }
}

Assert-Authenticode $Installer
$UninstallRoot = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
$Existing = @(Get-ChildItem $UninstallRoot -ErrorAction SilentlyContinue |
    ForEach-Object { Get-ItemProperty $_.PSPath } |
    Where-Object { $_.DisplayName -eq "MDViewer" })
if ($Existing.Count -ne 0) {
    throw "Windows install smoke requires MDViewer to be absent"
}

$Install = Start-Process -FilePath $Installer -ArgumentList "/S" -Wait -PassThru
if ($Install.ExitCode -ne 0) {
    throw "NSIS silent installation failed"
}
$Entry = @(Get-ChildItem $UninstallRoot -ErrorAction Stop |
    ForEach-Object { Get-ItemProperty $_.PSPath } |
    Where-Object { $_.DisplayName -eq "MDViewer" -and $_.DisplayVersion -eq $Receipt.version })
if ($Entry.Count -ne 1) {
    throw "Installed MDViewer registry entry was not found"
}
$InstallLocation = [string]$Entry[0].InstallLocation
if (-not (Test-Path $InstallLocation -PathType Container)) {
    throw "Installed MDViewer directory was not found"
}
$Executable = @(Get-ChildItem $InstallLocation -Filter "*.exe" -File |
    Where-Object { $_.Name -notmatch '(?i)uninstall' })[0].FullName
Assert-Authenticode $Executable

$Process = Start-Process -FilePath $Executable -PassThru
Start-Sleep -Seconds 5
if ($Process.HasExited) {
    throw "Installed MDViewer did not remain running"
}
Stop-Process -Id $Process.Id -Force
$Process.WaitForExit()

$UninstallString = [string]$Entry[0].UninstallString
$Match = [regex]::Match($UninstallString, '^"([^"]+)"|^(\S+)')
$Uninstaller = if ($Match.Groups[1].Success) { $Match.Groups[1].Value } else { $Match.Groups[2].Value }
if (-not (Test-Path $Uninstaller -PathType Leaf)) {
    throw "MDViewer uninstaller was not found"
}
$Uninstall = Start-Process -FilePath $Uninstaller -ArgumentList "/S" -Wait -PassThru
if ($Uninstall.ExitCode -ne 0) {
    throw "NSIS silent uninstall failed"
}
if (Get-ChildItem $UninstallRoot -ErrorAction SilentlyContinue |
    ForEach-Object { Get-ItemProperty $_.PSPath } |
    Where-Object { $_.DisplayName -eq "MDViewer" }) {
    throw "MDViewer uninstall registry entry remains"
}

$Receipt.publishable = $true
$Receipt.provenance = "github_attestation_required"
$TemporaryReceipt = "$ReceiptPath.tmp"
$Receipt | ConvertTo-Json -Depth 6 | Set-Content $TemporaryReceipt -Encoding utf8NoBOM
Move-Item $TemporaryReceipt $ReceiptPath -Force
Write-Host "WINDOWS RELEASE VERIFIED: Authenticode and install/launch/uninstall lifecycle pass."
