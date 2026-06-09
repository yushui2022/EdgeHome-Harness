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

    Write-Host "== 1. Eval: cases/zh-home.yaml =="
    $eval = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "eval", "cases/zh-home.yaml")
    $eval.report | ConvertTo-Json -Depth 8

    Write-Host "`n== 2. Dry-run: scheduled hallway brightness =="
    $dryRun = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandHallwaySchedule
    )
    $dryRun | ConvertTo-Json -Depth 12

    Write-Host "`n== 3. Replay dry-run trace =="
    $replay = Invoke-EdgeHomeJson @("--db-path", $DatabasePath, "replay", $dryRun.trace_id)
    $replay.replay_summary | ConvertTo-Json -Depth 12

    Write-Host "`n== 4. Dangerous action is blocked by gate =="
    $dangerous = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandGasAlarmOff
    )
    $dangerous | ConvertTo-Json -Depth 12

    Write-Host "`n== 5. Short memory resolves relative command =="
    $first = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandLivingRoomLightOff
    )
    $relative = Invoke-EdgeHomeJson @(
        "--db-path", $DatabasePath,
        "dry-run", "--mock",
        $CommandRelativeLightDarker
    )
    @{
        first_trace_id = $first.trace_id
        relative_trace_id = $relative.trace_id
        relative_normalized_command = $relative.normalized_command
        relative_policy_decision = $relative.policy_decision
    } | ConvertTo-Json -Depth 12

    Write-Host "`n== 6. low_memory profile =="
    $profile = Invoke-EdgeHomeJson @("config", "show")
    @{
        model_name = $profile.model_name
        num_ctx = $profile.num_ctx
        num_predict = $profile.num_predict
        max_short_memory_turns = $profile.max_short_memory_turns
        max_context_chars = $profile.max_context_chars
        executor_backend = $profile.executor_backend
    } | ConvertTo-Json -Depth 8
}
finally {
    Pop-Location
}
