<#
  One-time setup for signed updates.

  Marquee's updater only installs a download whose signature matches the public key built into
  the app. This script makes that key pair:

    1. creates the key pair (you choose a password),
    2. writes the PUBLIC key into src-tauri/tauri.conf.json (safe to commit),
    3. saves the PRIVATE key to %USERPROFILE%\.tauri\marquee.key (never commit this),
    4. if the GitHub CLI is signed in, stores the private key and password as repository secrets.

  Run it from the repository root:   powershell -ExecutionPolicy Bypass -File scripts\setup-signing.ps1

  Keep a copy of the private key and its password somewhere safe. If you lose them, installed
  copies can no longer be updated in place; people would have to download a new installer.
#>
$ErrorActionPreference = 'Stop'
$conf = 'src-tauri/tauri.conf.json'
if (-not (Test-Path $conf)) { throw 'Run this from the repository root (the folder with package.json).' }

$keyDir = Join-Path $env:USERPROFILE '.tauri'
$keyFile = Join-Path $keyDir 'marquee.key'
New-Item -ItemType Directory -Force -Path $keyDir | Out-Null

if (Test-Path $keyFile) {
  Write-Host "A key already exists at $keyFile. Using it. (Delete it first if you really want a new one; installed copies would stop accepting updates.)" -ForegroundColor Yellow
} else {
  $secure = Read-Host 'Choose a password for the signing key' -AsSecureString
  $password = [Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
  if (-not $password) { throw 'A password is required.' }
  npx tauri signer generate --ci -w $keyFile -p $password | Out-Null
  if (-not (Test-Path $keyFile)) { throw 'The key was not created. Is Node installed, and did "npm ci" run?' }
  $script:password = $password
}

$public = (Get-Content "$keyFile.pub" -Raw).Trim()
$json = Get-Content $conf -Raw
$json = [regex]::Replace($json, '"pubkey":\s*"[^"]*"', ('"pubkey": "' + $public + '"'))
Set-Content -Path $conf -Value $json -NoNewline
Write-Host "Public key written to $conf. Commit that change." -ForegroundColor Green

if (Get-Command gh -ErrorAction SilentlyContinue) {
  if (-not $script:password) {
    $secure = Read-Host 'Password of the existing key (to store it as a GitHub secret)' -AsSecureString
    $script:password = [Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
  }
  Get-Content $keyFile -Raw | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo larosa023/Marquee
  $script:password | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo larosa023/Marquee
  Write-Host 'Stored TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD as repository secrets.' -ForegroundColor Green
} else {
  Write-Host ''
  Write-Host 'GitHub CLI not found. Add these two secrets by hand at' -ForegroundColor Yellow
  Write-Host '  https://github.com/larosa023/Marquee/settings/secrets/actions' -ForegroundColor Yellow
  Write-Host "  TAURI_SIGNING_PRIVATE_KEY           the full contents of $keyFile"
  Write-Host '  TAURI_SIGNING_PRIVATE_KEY_PASSWORD  the password you just chose'
}
Write-Host ''
Write-Host "Private key: $keyFile  (back this up; never commit it)"
