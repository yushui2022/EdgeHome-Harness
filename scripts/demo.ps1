param(
    [string]$DatabasePath = "edgehome-demo.sqlite",
    [string]$OutputDir = ""
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $RepoRoot

try {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP "edgehome-target"

    $Artifacts = New-Object System.Collections.Generic.List[object]
    $ReportLines = New-Object System.Collections.Generic.List[string]
    $ResolvedOutputDir = $null

    if (-not [string]::IsNullOrWhiteSpace($OutputDir)) {
        $ResolvedOutputDir = Join-Path $RepoRoot $OutputDir
        New-Item -ItemType Directory -Force -Path $ResolvedOutputDir | Out-Null
    }

    function New-Utf16Text {
        param(
            [Parameter(Mandatory = $true)]
            [int[]]$CodePoints
        )

        return -join ($CodePoints | ForEach-Object { [char]$_ })
    }

    function Invoke-EdgeHomeJson {
        param(
            [Parameter(Mandatory = $true)]
            [string[]]$CommandArgs
        )

        $output = & cargo run -q -p edgehome-cli -- @CommandArgs | Out-String
        if ($LASTEXITCODE -ne 0) {
            throw "edgehome command failed: $($CommandArgs -join ' ')"
        }

        return $output | ConvertFrom-Json
    }

    function ConvertTo-DemoJson {
        param(
            [Parameter(Mandatory = $true)]
            $Value
        )

        return $Value | ConvertTo-Json -Depth 24
    }

    function Save-DemoJson {
        param(
            [Parameter(Mandatory = $true)]
            [string]$FileName,
            [Parameter(Mandatory = $true)]
            $Value,
            [Parameter(Mandatory = $true)]
            [string]$Description
        )

        if ($null -eq $ResolvedOutputDir) {
            return
        }

        $path = Join-Path $ResolvedOutputDir $FileName
        ConvertTo-DemoJson $Value | Set-Content -Encoding UTF8 -LiteralPath $path
        $Artifacts.Add([pscustomobject]@{
            File = $FileName
            Description = $Description
        }) | Out-Null
    }

    function Save-DemoText {
        param(
            [Parameter(Mandatory = $true)]
            [string]$FileName,
            [Parameter(Mandatory = $true)]
            [string]$Text,
            [Parameter(Mandatory = $true)]
            [string]$Description
        )

        if ($null -eq $ResolvedOutputDir) {
            return
        }

        $path = Join-Path $ResolvedOutputDir $FileName
        $Text | Set-Content -Encoding UTF8 -LiteralPath $path
        $Artifacts.Add([pscustomobject]@{
            File = $FileName
            Description = $Description
        }) | Out-Null
    }

    function Add-ReportLine {
        param([string]$Line = "")
        $ReportLines.Add($Line) | Out-Null
    }

    function Write-DemoJson {
        param(
            [Parameter(Mandatory = $true)]
            $Value
        )

        ConvertTo-DemoJson $Value
    }

    function Invoke-NativeText {
        param(
            [Parameter(Mandatory = $true)]
            [string]$Command,
            [Parameter(Mandatory = $true)]
            [string[]]$Arguments
        )

        function ConvertTo-ProcessArgument {
            param([string]$Argument)

            if ($Argument.Length -eq 0) {
                return '""'
            }
            if ($Argument -notmatch '[\s"]') {
                return $Argument
            }

            return '"' + ($Argument -replace '"', '\"') + '"'
        }

        $process = New-Object System.Diagnostics.Process
        $process.StartInfo.FileName = $Command
        $process.StartInfo.Arguments = ($Arguments | ForEach-Object { ConvertTo-ProcessArgument $_ }) -join " "
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.RedirectStandardOutput = $true
        $process.StartInfo.RedirectStandardError = $true
        $process.StartInfo.CreateNoWindow = $true

        $process.Start() | Out-Null
        $stdout = $process.StandardOutput.ReadToEnd()
        $stderr = $process.StandardError.ReadToEnd()
        $process.WaitForExit()

        $outputParts = @()
        if (-not [string]::IsNullOrWhiteSpace($stderr)) {
            $outputParts += $stderr.TrimEnd()
        }
        if (-not [string]::IsNullOrWhiteSpace($stdout)) {
            $outputParts += $stdout.TrimEnd()
        }
        $output = $outputParts -join "`n"

        if ($process.ExitCode -ne 0) {
            throw "$Command failed with exit code $($process.ExitCode)`n$output"
        }

        return $output
    }

    $CommandHallwaySchedule = New-Utf16Text @(26202,19978,21313,28857,21518,25226,36208,24266,28783,35843,21040,51,48,37)
    $CommandGasAlarmOff = New-Utf16Text @(20851,38381,29123,27668,25253,35686,22120)
    $CommandLivingRoomLightOff = New-Utf16Text @(25226,23458,21381,28783,20851,25481)
    $CommandRelativeLightDarker = New-Utf16Text @(25226,21018,25165,37027,20010,28783,20877,35843,26263,19968,28857)
    $CommandRememberHallwayAlias = New-Utf16Text @(20197,21518,25226,29572,20851,28783,21483,23567,22812,28783)
    $CommandAliasLightOn = New-Utf16Text @(25171,24320,23567,22812,28783)

    Add-ReportLine "# EdgeHome Harness Public Demo Report"
    Add-ReportLine ""
    Add-ReportLine "- Generated at: $((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))"
    Add-ReportLine "- Database path: ``$DatabasePath``"
    Add-ReportLine "- Model mode: mock"
    Add-ReportLine "- Real device execution: disabled by default"
    Add-ReportLine ""

    Write-Host "== 1. Release gate: cases/zh-home.yaml =="
    $eval = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "eval", "cases/zh-home.yaml", "--gate")
    $evalSummary = [ordered]@{
        report = $eval.report
        gate = $eval.gate
    }
    Write-DemoJson $evalSummary
    Save-DemoJson "01-release-gate.json" $evalSummary "Mock release eval gate and metrics."

    Add-ReportLine "## Release Gate"
    Add-ReportLine ""
    Add-ReportLine "- Gate passed: ``$($eval.gate.passed)``"
    Add-ReportLine "- Total cases: ``$($eval.report.total)``"
    Add-ReportLine "- Category count: ``$($eval.report.category_count)``"
    Add-ReportLine "- Pass rate: ``$($eval.report.pass_rate)``"
    Add-ReportLine "- False allow rate: ``$($eval.report.false_allow_rate)``"
    Add-ReportLine "- Fail-closed rate: ``$($eval.report.fail_closed_rate)``"
    Add-ReportLine "- Trace coverage: ``$($eval.report.trace_coverage)``"
    Add-ReportLine ""

    Write-Host "`n== 2. Ordinary command: living room light off =="
    $ordinary = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandLivingRoomLightOff
    )
    Write-DemoJson $ordinary
    Save-DemoJson "02-ordinary-dry-run.json" $ordinary "Ordinary command dry-run output."

    Add-ReportLine "## Ordinary Command"
    Add-ReportLine ""
    Add-ReportLine "- Trace ID: ``$($ordinary.trace_id)``"
    Add-ReportLine "- Mode: ``$($ordinary.mode)``"
    Add-ReportLine "- Backend: ``$($ordinary.dry_run_plan.backend)``"
    Add-ReportLine "- Target: ``$($ordinary.dry_run_plan.plan.target)``"
    Add-ReportLine "- Action: ``$($ordinary.dry_run_plan.plan.action)``"
    Add-ReportLine ""

    Write-Host "`n== 3. Slot extraction: scheduled hallway brightness =="
    $dryRun = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandHallwaySchedule
    )
    Write-DemoJson $dryRun
    Save-DemoJson "03-slot-dry-run.json" $dryRun "Scheduled hallway brightness dry-run output."

    Add-ReportLine "## Slot Extraction"
    Add-ReportLine ""
    Add-ReportLine "- Trace ID: ``$($dryRun.trace_id)``"
    Add-ReportLine "- Target: ``$($dryRun.dry_run_plan.plan.target)``"
    Add-ReportLine "- Action: ``$($dryRun.dry_run_plan.plan.action)``"
    Add-ReportLine "- Brightness: ``$($dryRun.dry_run_plan.plan.params.brightness)``"
    Add-ReportLine "- Time after: ``$($dryRun.dry_run_plan.plan.params.time_after)``"
    Add-ReportLine ""

    Write-Host "`n== 4. Replay and export dry-run trace =="
    $replay = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "replay", $dryRun.trace_id)
    $traceFrame = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "trace", "export", $dryRun.trace_id)
    $traceEvidence = [ordered]@{
        replay_summary = $replay.replay_summary
        trace_frame = $traceFrame
    }
    Write-DemoJson $traceEvidence
    Save-DemoJson "04-replay-summary.json" $replay "Replay output for the slot extraction trace."
    Save-DemoJson "05-trace-frame.json" $traceFrame "TraceFrame export for the slot extraction trace."

    Add-ReportLine "## Trace Replay"
    Add-ReportLine ""
    Add-ReportLine "- Replayed trace ID: ``$($dryRun.trace_id)``"
    Add-ReportLine "- Gate count: ``$($replay.replay_summary.gate_count)``"
    Add-ReportLine "- Audit count: ``$($replay.replay_summary.audit_count)``"
    Add-ReportLine "- Trace frame step count: ``$($traceFrame.step_count)``"
    Add-ReportLine ""

    Write-Host "`n== 5. Short memory resolves relative command =="
    $relative = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandRelativeLightDarker
    )
    $shortMemory = [ordered]@{
        first_trace_id = $dryRun.trace_id
        relative_trace_id = $relative.trace_id
        relative_normalized_command = $relative.normalized_command
        relative_policy_decision = $relative.policy_decision
    }
    Write-DemoJson $shortMemory
    Save-DemoJson "06-short-memory.json" $shortMemory "Relative-command short-memory resolution evidence."

    Add-ReportLine "## Short Memory"
    Add-ReportLine ""
    Add-ReportLine "- Prior trace ID: ``$($dryRun.trace_id)``"
    Add-ReportLine "- Relative trace ID: ``$($relative.trace_id)``"
    Add-ReportLine "- Resolved device: ``$($relative.normalized_command.device_id)``"
    Add-ReportLine ""

    Write-Host "`n== 6. Long memory alias write and resolution =="
    $memoryWrite = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandRememberHallwayAlias
    )
    $aliasUse = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandAliasLightOn
    )
    $longMemory = [ordered]@{
        memory_write = $memoryWrite.memory_write
        alias_trace_id = $aliasUse.trace_id
        alias_normalized_command = $aliasUse.normalized_command
    }
    Write-DemoJson $longMemory
    Save-DemoJson "07-long-memory.json" $longMemory "Explicit long-memory alias write and reuse evidence."

    Add-ReportLine "## Long Memory"
    Add-ReportLine ""
    Add-ReportLine "- Memory write accepted: ``$($null -ne $memoryWrite.memory_write)``"
    Add-ReportLine "- Alias trace ID: ``$($aliasUse.trace_id)``"
    Add-ReportLine "- Alias resolved device: ``$($aliasUse.normalized_command.device_id)``"
    Add-ReportLine ""

    Write-Host "`n== 7. Dangerous action is blocked by gate =="
    $dangerous = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandGasAlarmOff
    )
    Write-DemoJson $dangerous
    Save-DemoJson "08-dangerous-blocked.json" $dangerous "Dangerous action fail-closed evidence."

    $dangerousReason = $dangerous.failure_reason
    if ([string]::IsNullOrWhiteSpace($dangerousReason) -and $dangerous.gate_evaluation.blocking_reasons) {
        $dangerousReason = $dangerous.gate_evaluation.blocking_reasons -join "; "
    }
    $dangerousReasonForReport = $dangerousReason -replace '`', "'"

    Add-ReportLine "## Dangerous Action"
    Add-ReportLine ""
    Add-ReportLine "- Trace ID: ``$($dangerous.trace_id)``"
    Add-ReportLine "- Dry-run plan present: ``$($null -ne $dangerous.dry_run_plan)``"
    Add-ReportLine "- Failure reason: ``$dangerousReasonForReport``"
    Add-ReportLine ""

    Write-Host "`n== 8. low_memory pressure degradation =="
    $pressureNormal = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "1024")
    $pressureElevated = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "400")
    $pressureCritical = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "128")
    $pressure = [ordered]@{
        normal = $pressureNormal.decision
        elevated = $pressureElevated.decision
        critical = $pressureCritical.decision
    }
    Write-DemoJson $pressure
    Save-DemoJson "09-low-memory-pressure.json" $pressure "Runtime pressure decisions for low-memory operation."

    Add-ReportLine "## Low-Memory Pressure"
    Add-ReportLine ""
    Add-ReportLine "- Normal fallback: ``$($pressure.normal.fallback_mode)``"
    Add-ReportLine "- Elevated fallback: ``$($pressure.elevated.fallback_mode)``"
    Add-ReportLine "- Critical fallback: ``$($pressure.critical.fallback_mode)``"
    Add-ReportLine "- Critical memory enabled: ``$($pressure.critical.memory_enabled)``"
    Add-ReportLine ""

    Write-Host "`n== 9. Backend readiness checks =="
    $backendReadiness = [ordered]@{
        home_assistant = Invoke-EdgeHomeJson @("backend", "check", "--backend", "home_assistant", "--registry", "configs/devices.home_assistant.example.yaml")
        mqtt = Invoke-EdgeHomeJson @("backend", "check", "--backend", "mqtt", "--registry", "configs/devices.mqtt.example.yaml")
        miot = Invoke-EdgeHomeJson @("backend", "check", "--backend", "miot", "--registry", "configs/devices.miot.example.yaml")
        matter = Invoke-EdgeHomeJson @("backend", "check", "--backend", "matter", "--registry", "configs/devices.matter.example.yaml")
    }
    Write-DemoJson $backendReadiness
    Save-DemoJson "10-backend-readiness.json" $backendReadiness "Read-only backend readiness checks."

    Add-ReportLine "## Backend Readiness"
    Add-ReportLine ""
    foreach ($name in @("home_assistant", "mqtt", "miot", "matter")) {
        $check = $backendReadiness[$name].checks[0]
        Add-ReportLine "- ${name}: dry_run_ready=``$($check.dry_run_ready)``, execute_enabled=``$($check.execute_enabled)``, execute_ready=``$($check.execute_ready)``"
    }
    Add-ReportLine ""

    Write-Host "`n== 10. OutputGovernor dead-loop fallback test =="
    $governorOutput = Invoke-NativeText "cargo" @(
        "test",
        "-q",
        "-p",
        "edgehome-ollama",
        "output_governor_report_classifies_dead_loop_and_fallback"
    )
    Write-Host $governorOutput
    Save-DemoText "11-output-governor-test.txt" $governorOutput "OutputGovernor focused test output."

    Add-ReportLine "## Output Governor"
    Add-ReportLine ""
    Add-ReportLine "- Focused test: ``output_governor_report_classifies_dead_loop_and_fallback``"
    Add-ReportLine "- Result: passed"
    Add-ReportLine ""

    if ($null -ne $ResolvedOutputDir) {
        Add-ReportLine "## Artifact Index"
        Add-ReportLine ""
        Add-ReportLine "| File | Description |"
        Add-ReportLine "| --- | --- |"
        foreach ($artifact in $Artifacts) {
            Add-ReportLine "| ``$($artifact.File)`` | $($artifact.Description) |"
        }
        Add-ReportLine ""
        Add-ReportLine "## Claim Boundary"
        Add-ReportLine ""
        Add-ReportLine "This demo evidence uses the mock model path and mock/default backend configs."
        Add-ReportLine "It proves the harness pipeline, gate behavior, trace/replay, eval gate, and"
        Add-ReportLine "adapter readiness boundaries. It does not prove universal smart-home support,"
        Add-ReportLine "real Xiaomi device validation, real Matter device validation, or default-on real"
        Add-ReportLine "device execution."
        Add-ReportLine ""

        $reportPath = Join-Path $ResolvedOutputDir "public-demo-report.md"
        $ReportLines -join "`n" | Set-Content -Encoding UTF8 -LiteralPath $reportPath
        Write-Host ""
        Write-Host "Demo evidence written to: $ResolvedOutputDir" -ForegroundColor Green
    }
}
finally {
    Pop-Location
}
