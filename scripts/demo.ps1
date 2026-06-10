param(
    [string]$DatabasePath = "edgehome-demo.sqlite"
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $RepoRoot

try {
    $env:CARGO_TARGET_DIR = Join-Path $env:TEMP "edgehome-target"

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
        return $output | ConvertFrom-Json
    }

    $CommandHallwaySchedule = New-Utf16Text @(26202,19978,21313,28857,21518,25226,36208,24266,28783,35843,21040,51,48,37)
    $CommandGasAlarmOff = New-Utf16Text @(20851,38381,29123,27668,25253,35686,22120)
    $CommandLivingRoomLightOff = New-Utf16Text @(25226,23458,21381,28783,20851,25481)
    $CommandRelativeLightDarker = New-Utf16Text @(25226,21018,25165,37027,20010,28783,20877,35843,26263,19968,28857)
    $CommandRememberHallwayAlias = New-Utf16Text @(20197,21518,25226,29572,20851,28783,21483,23567,22812,28783)
    $CommandAliasLightOn = New-Utf16Text @(25171,24320,23567,22812,28783)

    Write-Host "== 1. Release gate: cases/zh-home.yaml =="
    $eval = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "eval", "cases/zh-home.yaml", "--gate")
    @{
        report = $eval.report
        gate = $eval.gate
    } | ConvertTo-Json -Depth 12

    Write-Host "`n== 2. Ordinary command: living room light off =="
    $ordinary = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandLivingRoomLightOff
    )
    $ordinary | ConvertTo-Json -Depth 12

    Write-Host "`n== 3. Slot extraction: scheduled hallway brightness =="
    $dryRun = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandHallwaySchedule
    )
    $dryRun | ConvertTo-Json -Depth 12

    Write-Host "`n== 4. Replay and export dry-run trace =="
    $replay = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "replay", $dryRun.trace_id)
    $traceFrame = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "trace", "export", $dryRun.trace_id)
    @{
        replay_summary = $replay.replay_summary
        trace_frame = $traceFrame
    } | ConvertTo-Json -Depth 12

    Write-Host "`n== 5. Short memory resolves relative command =="
    $relative = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandRelativeLightDarker
    )
    @{
        first_trace_id = $dryRun.trace_id
        relative_trace_id = $relative.trace_id
        relative_normalized_command = $relative.normalized_command
        relative_policy_decision = $relative.policy_decision
    } | ConvertTo-Json -Depth 12

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
    @{
        memory_write = $memoryWrite.memory_write
        alias_trace_id = $aliasUse.trace_id
        alias_normalized_command = $aliasUse.normalized_command
    } | ConvertTo-Json -Depth 12

    Write-Host "`n== 7. Dangerous action is blocked by gate =="
    $dangerous = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandGasAlarmOff
    )
    $dangerous | ConvertTo-Json -Depth 12

    Write-Host "`n== 8. low_memory pressure degradation =="
    $pressureNormal = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "1024")
    $pressureElevated = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "400")
    $pressureCritical = Invoke-EdgeHomeJson @("config", "pressure", "--free-memory-mb", "128")
    @{
        normal = $pressureNormal.decision
        elevated = $pressureElevated.decision
        critical = $pressureCritical.decision
    } | ConvertTo-Json -Depth 12

    Write-Host "`n== 9. OutputGovernor dead-loop fallback test =="
    & cargo test -q -p edgehome-ollama output_governor_report_classifies_dead_loop_and_fallback
}
finally {
    Pop-Location
}
