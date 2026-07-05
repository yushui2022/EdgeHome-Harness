param(
    [string[]]$Paths = @(
        "README.md",
        "CHANGELOG.md",
        "docs\README.md",
        "docs\waic-one-page.md",
        "docs\roadmap.md",
        "docs\release-checklist.md",
        "docs\release-evidence.md",
        "docs\home-assistant-gateway.md",
        "docs\mqtt-guarded-publish.md",
        "docs\miot-bridge-adapter.md",
        "docs\matter-bridge-adapter.md",
        "docs\real-minicpm-eval-report.md"
    )
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $RepoRoot

try {
    $rules = @(
        [pscustomobject]@{
            Name = "Universal Xiaomi/MIoT support claim"
            Pattern = '(?i)\b(fully|universally|production[- ]ready)\s+supports?\s+(xiaomi|miot)\b'
            Guidance = "Claim MIoT/Xiaomi as bridge-adapter support until private bridge and real-device evidence exist."
        },
        [pscustomobject]@{
            Name = "Universal Matter support claim"
            Pattern = '(?i)\b(fully|universally|production[- ]ready)\s+supports?\s+matter\b'
            Guidance = "Claim Matter as controller-bridge adapter support until controller and real-device evidence exist."
        },
        [pscustomobject]@{
            Name = "Default real-device execution claim"
            Pattern = '(?i)\breal[- ]device execution\s+(is\s+)?enabled by default\b'
            Guidance = "Public materials must say real-device execution is disabled by default and explicit opt-in only."
        },
        [pscustomobject]@{
            Name = "Vendor-ready model JSON claim"
            Pattern = '(?i)\b(MiniCPM|model)\s+(emits|outputs|generates|produces)\s+vendor[- ]ready\s+JSON\b'
            Guidance = "The model emits backend-neutral candidate JSON; adapters generate backend payloads."
        },
        [pscustomobject]@{
            Name = "Production smart-home gateway claim"
            Pattern = '(?i)\b(is|as|the)\s+(a\s+)?production[- ]ready\s+smart[- ]home gateway\b'
            Guidance = "Use safety harness, evaluation prototype, or gateway boundary unless production hardening evidence exists."
        },
        [pscustomobject]@{
            Name = "Home Assistant replacement claim"
            Pattern = '(?i)\b(is|as|the)\s+(a\s+)?Home Assistant replacement\b'
            Guidance = "EdgeHome Harness integrates through a Home Assistant gateway boundary; it does not replace Home Assistant."
        },
        [pscustomobject]@{
            Name = "Mi Home replacement claim"
            Pattern = '(?i)\b(is|as|the)\s+(a\s+)?Mi Home replacement\b'
            Guidance = "Do not position the project as a Mi Home replacement."
        }
    )

    $failures = New-Object System.Collections.Generic.List[string]

    foreach ($path in $Paths) {
        if (-not (Test-Path -LiteralPath $path)) {
            $failures.Add("missing release-facing document: $path")
            continue
        }

        $lines = Get-Content -Encoding UTF8 -LiteralPath $path
        $negativeContext = $false
        for ($index = 0; $index -lt $lines.Count; $index++) {
            $line = $lines[$index]
            if ($line -match 'public-claim-allow:') {
                continue
            }
            if ($line -match '^\s*#{1,6}\s+') {
                $negativeContext = $line -match '(?i)(not claimed|claims not allowed|not implemented|not included|non-goals|documentation rules)'
            }
            elseif ($line -match '(?i)^\s*(Not claimed|Not implemented today|Not Included|Unsafe claim|Documentation rules):\s*$') {
                $negativeContext = $true
            }

            if ($negativeContext) {
                continue
            }
            if ($line -match '(?i)\b(do not|does not|not a|not as|not claim|not proof|not prove|without)\b') {
                continue
            }

            foreach ($rule in $rules) {
                if ($line -match $rule.Pattern) {
                    $lineNumber = $index + 1
                    $failures.Add("${path}:$lineNumber [$($rule.Name)] $($rule.Guidance) :: $line")
                }
            }
        }
    }

    if ($failures.Count -gt 0) {
        $failures | ForEach-Object { Write-Host "FAIL: $_" -ForegroundColor Red }
        throw "public claim lint failed"
    }

    [pscustomobject]@{
        status = "passed"
        checked_files = $Paths.Count
        rules = $rules.Count
    } | ConvertTo-Json
}
finally {
    Pop-Location
}
