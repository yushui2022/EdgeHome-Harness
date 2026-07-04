param(
    [string]$DatabasePath = "",
    [switch]$NoLocked,
    [switch]$SkipEval,
    [switch]$SkipHygieneScan
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $RepoRoot

try {
    if ([string]::IsNullOrWhiteSpace($DatabasePath)) {
        $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
        $DatabasePath = Join-Path $env:TEMP "edgehome-release-check-$timestamp.sqlite"
    }

    $CargoLockArgs = @()
    if (-not $NoLocked) {
        $CargoLockArgs += "--locked"
    }

    function Invoke-Native {
        param(
            [Parameter(Mandatory = $true)]
            [string]$Label,
            [Parameter(Mandatory = $true)]
            [string[]]$Command
        )

        Write-Host ""
        Write-Host "== $Label ==" -ForegroundColor Cyan
        Write-Host ("> " + ($Command -join " "))

        $exe = $Command[0]
        $args = @()
        if ($Command.Length -gt 1) {
            $args = $Command[1..($Command.Length - 1)]
        }

        & $exe @args
        if ($LASTEXITCODE -ne 0) {
            throw "$Label failed with exit code $LASTEXITCODE"
        }
    }

    function Get-TrackedFiles {
        $files = & git ls-files
        if ($LASTEXITCODE -ne 0) {
            throw "git ls-files failed"
        }
        return $files
    }

    function Assert-ReleaseHygiene {
        Write-Host ""
        Write-Host "== Release hygiene scan ==" -ForegroundColor Cyan

        $tracked = Get-TrackedFiles
        $failures = New-Object System.Collections.Generic.List[string]

        foreach ($path in $tracked) {
            $normalized = $path -replace '\\', '/'
            if ($normalized -match '(^|/)artifacts/') {
                $failures.Add("tracked artifact file: $path")
            }
            if ($normalized -match '(^|/)\.env($|\.)') {
                $failures.Add("tracked environment file: $path")
            }
            if ($normalized -match '\.(db|sqlite|sqlite3|log)$') {
                $failures.Add("tracked runtime output file: $path")
            }

            $item = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
            if ($null -eq $item -or $item.Length -gt 2MB) {
                continue
            }

            $extension = [System.IO.Path]::GetExtension($path).ToLowerInvariant()
            if ($extension -in @(".jpg", ".jpeg", ".png", ".gif", ".ico", ".pdf")) {
                continue
            }

            $content = Get-Content -Raw -Encoding UTF8 -LiteralPath $path -ErrorAction SilentlyContinue
            if ($null -eq $content) {
                continue
            }

            if ($content -cmatch '-----BEGIN (RSA |EC |OPENSSH |)PRIVATE KEY-----') {
                $failures.Add("private key marker found in tracked file: $path")
            }

            if (
                $normalized -match '^configs/' -and
                $normalized -match '\.example\.' -and
                $content -match '(?m)^\s*execute_enabled:\s*true\s*$'
            ) {
                $failures.Add("public example config enables real execution: $path")
            }
        }

        if ($failures.Count -gt 0) {
            $failures | ForEach-Object { Write-Host "FAIL: $_" -ForegroundColor Red }
            throw "release hygiene scan failed"
        }

        Write-Host "release hygiene scan passed"
    }

    function Assert-JsonSchemaFiles {
        Write-Host ""
        Write-Host "== JSON schema syntax ==" -ForegroundColor Cyan

        $schemaDir = Join-Path $RepoRoot "docs\schemas"
        if (-not (Test-Path -LiteralPath $schemaDir)) {
            Write-Host "no docs\schemas directory found"
            return
        }

        $files = Get-ChildItem -LiteralPath $schemaDir -Filter "*.json" -File
        foreach ($file in $files) {
            try {
                Get-Content -Raw -Encoding UTF8 -LiteralPath $file.FullName | ConvertFrom-Json | Out-Null
            }
            catch {
                throw "invalid JSON schema file $($file.FullName): $($_.Exception.Message)"
            }
        }

        Write-Host "JSON schema syntax passed"
    }

    if (-not $SkipHygieneScan) {
        Assert-ReleaseHygiene
    }
    Assert-JsonSchemaFiles

    Invoke-Native "Format" @("cargo", "fmt", "--all", "--check")
    Invoke-Native "Clippy" (@("cargo", "clippy") + $CargoLockArgs + @("--workspace", "--all-targets", "--", "-D", "warnings"))
    Invoke-Native "Workspace tests" (@("cargo", "test") + $CargoLockArgs + @("--workspace"))

    if (-not $SkipEval) {
        Invoke-Native "Release eval gate" (@(
            "cargo", "run"
        ) + $CargoLockArgs + @(
            "-q", "-p", "edgehome-cli", "--",
            "--db-path", $DatabasePath,
            "eval", "cases\zh-home.yaml", "--gate"
        ))
    }

    Invoke-Native "Backend check: Home Assistant" (@(
        "cargo", "run"
    ) + $CargoLockArgs + @(
        "-q", "-p", "edgehome-cli", "--",
        "backend", "check", "--backend", "home_assistant",
        "--registry", "configs\devices.home_assistant.example.yaml"
    ))
    Invoke-Native "Backend check: MQTT" (@(
        "cargo", "run"
    ) + $CargoLockArgs + @(
        "-q", "-p", "edgehome-cli", "--",
        "backend", "check", "--backend", "mqtt",
        "--registry", "configs\devices.mqtt.example.yaml"
    ))
    Invoke-Native "Backend check: MIoT bridge" (@(
        "cargo", "run"
    ) + $CargoLockArgs + @(
        "-q", "-p", "edgehome-cli", "--",
        "backend", "check", "--backend", "miot",
        "--registry", "configs\devices.miot.example.yaml"
    ))
    Invoke-Native "Backend check: Matter bridge" (@(
        "cargo", "run"
    ) + $CargoLockArgs + @(
        "-q", "-p", "edgehome-cli", "--",
        "backend", "check", "--backend", "matter",
        "--registry", "configs\devices.matter.example.yaml"
    ))

    Invoke-Native "Diff whitespace check" @("git", "diff", "--check")

    Write-Host ""
    Write-Host "Release check passed." -ForegroundColor Green
}
finally {
    Pop-Location
}
