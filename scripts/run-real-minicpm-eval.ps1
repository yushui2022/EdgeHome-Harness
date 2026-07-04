param(
    [string]$ModelName = "openbmb/minicpm5:latest",
    [string]$Profile = "eval_mode",
    [string]$CasesPath = "cases\zh-home.yaml",
    [string]$OutputDir = "artifacts",
    [string]$DatabasePath = "",
    [int]$TimeoutMs = 60000,
    [int]$NumPredict = 128
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
if ($TimeoutMs -le 0) {
    throw "TimeoutMs must be greater than 0"
}
if ($NumPredict -le 0) {
    throw "NumPredict must be greater than 0"
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
$installedModels = $modelList | Select-Object -Skip 1 | ForEach-Object { ($_ -split '\s+')[0] }
if ($installedModels -notcontains $ModelName) {
    Write-Step "model tag not found in 'ollama list'; attempting 'ollama pull $ModelName'"
    ollama pull $ModelName
    if ($LASTEXITCODE -ne 0) {
        throw "ollama pull failed for $ModelName"
    }
}

Write-Step "writing metadata to $metadataPath"
@(
    "date=$(Get-Date -Format o)"
    "os=$([System.Environment]::OSVersion.VersionString)"
    "machine=$env:COMPUTERNAME"
    "model=$ModelName"
    "profile=$Profile"
    "cases=$CasesPath"
    "timeout_ms=$TimeoutMs"
    "num_predict=$NumPredict"
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
$quotedModelName = "'" + ($ModelName -replace "'", "''") + "'"
if ($profileContent -match '(?m)^model_name:\s*.+$') {
    $profileContent = $profileContent -replace '(?m)^model_name:\s*.+$', "model_name: $quotedModelName"
} else {
    $profileContent = $profileContent.TrimEnd() + "`nmodel_name: $quotedModelName`n"
}
if ($profileContent -match '(?m)^timeout_ms:\s*.+$') {
    $profileContent = $profileContent -replace '(?m)^timeout_ms:\s*.+$', "timeout_ms: $TimeoutMs"
} else {
    $profileContent = $profileContent.TrimEnd() + "`ntimeout_ms: $TimeoutMs`n"
}
if ($profileContent -match '(?m)^num_predict:\s*.+$') {
    $profileContent = $profileContent -replace '(?m)^num_predict:\s*.+$', "num_predict: $NumPredict"
} else {
    $profileContent = $profileContent.TrimEnd() + "`nnum_predict: $NumPredict`n"
}
$utf8NoBom = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($profilePath, $profileContent, $utf8NoBom)

Write-Step "validating temporary profile"
cargo run -q -p edgehome-cli -- --config-dir $tempConfigDir --profile $Profile config show | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "temporary profile validation failed"
}

Write-Step "running real MiniCPM/Ollama eval"
cargo run -q -p edgehome-cli -- --config-dir $tempConfigDir --profile $Profile --db-path $DatabasePath eval $CasesPath --ollama |
    Tee-Object -FilePath $outputPath
if ($LASTEXITCODE -ne 0) {
    throw "real MiniCPM/Ollama eval failed"
}

Write-Step "raw eval JSON written to $outputPath"
Write-Step "metadata written to $metadataPath"
Write-Step "review raw output before copying stable metrics into docs/real-minicpm-eval-report.md"
