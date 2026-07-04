param(
    [string]$ModelName = "openbmb/minicpm5:1b",
    [string]$Profile = "eval_mode",
    [string]$CasesPath = "cases\zh-home.yaml",
    [string]$OutputDir = "artifacts",
    [string]$DatabasePath = ""
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "[edgehome-real-eval] $Message"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

if (-not (Get-Command ollama -ErrorAction SilentlyContinue)) {
    throw "ollama is not available on PATH"
}

if (-not (Test-Path $CasesPath)) {
    throw "cases file not found: $CasesPath"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
if ([string]::IsNullOrWhiteSpace($DatabasePath)) {
    $DatabasePath = Join-Path $env:TEMP "edgehome-real-minicpm-$timestamp.sqlite"
}

$safeModelName = $ModelName -replace '[^A-Za-z0-9_.-]', '_'
$outputPath = Join-Path $OutputDir "real-minicpm-eval-$safeModelName-$timestamp.json"
$metadataPath = Join-Path $OutputDir "real-minicpm-eval-$safeModelName-$timestamp.meta.txt"
$tempConfigDir = Join-Path $env:TEMP "edgehome-real-eval-config-$timestamp"

Write-Step "checking Ollama model: $ModelName"
$modelList = ollama list
if ($modelList -notmatch [regex]::Escape($ModelName)) {
    Write-Step "model tag not found in 'ollama list'; attempting 'ollama pull $ModelName'"
    ollama pull $ModelName
}

Write-Step "writing metadata to $metadataPath"
@(
    "date=$(Get-Date -Format o)"
    "os=$([System.Environment]::OSVersion.VersionString)"
    "machine=$env:COMPUTERNAME"
    "model=$ModelName"
    "profile=$Profile"
    "cases=$CasesPath"
    "db_path=$DatabasePath"
    "ollama_version=$(ollama --version)"
    "rustc=$(rustc --version)"
    "cargo=$(cargo --version)"
) | Set-Content -Encoding UTF8 $metadataPath

Write-Step "creating temporary config profile in $tempConfigDir"
New-Item -ItemType Directory -Force -Path $tempConfigDir | Out-Null
Copy-Item -Path (Join-Path $repoRoot "configs\*.yaml") -Destination $tempConfigDir -Force
$profilePath = Join-Path $tempConfigDir "$Profile.yaml"
if (-not (Test-Path $profilePath)) {
    throw "profile config not found after copy: $profilePath"
}
$profileContent = Get-Content -Raw -Encoding UTF8 $profilePath
if ($profileContent -match '(?m)^model_name:\s*.+$') {
    $profileContent = $profileContent -replace '(?m)^model_name:\s*.+$', "model_name: $ModelName"
} else {
    $profileContent = $profileContent.TrimEnd() + "`nmodel_name: $ModelName`n"
}
Set-Content -Encoding UTF8 -Path $profilePath -Value $profileContent

Write-Step "running real MiniCPM/Ollama eval"
cargo run -q -p edgehome-cli -- --config-dir $tempConfigDir --profile $Profile --db-path $DatabasePath eval $CasesPath --ollama |
    Tee-Object -FilePath $outputPath

Write-Step "raw eval JSON written to $outputPath"
Write-Step "metadata written to $metadataPath"
Write-Step "review raw output before copying stable metrics into docs/real-minicpm-eval-report.md"
