# Runs the development server with a local data folder and the fake connector (synthetic libraries).
# The key and database live in ./.data, which git ignores.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$data = Join-Path $root '.data'
New-Item -ItemType Directory -Force $data | Out-Null
$key = Join-Path $data 'dev-key.txt'
if (-not (Test-Path $key)) {
    $bytes = New-Object byte[] 32
    [System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    [Convert]::ToBase64String($bytes) | Set-Content -NoNewline -Path $key
}
$env:GAMESHELF_SECRET_KEY_FILE = $key
$env:GAMESHELF_DATA_DIR = $data
if (-not $env:GAMESHELF_DEV_FAKE_CONNECTOR) { $env:GAMESHELF_DEV_FAKE_CONNECTOR = 'true' }
$env:GAMESHELF_LOG_FORMAT = 'pretty'
if (-not $env:GAMESHELF_LOG) { $env:GAMESHELF_LOG = 'info' }
Set-Location $root
cargo leptos watch
