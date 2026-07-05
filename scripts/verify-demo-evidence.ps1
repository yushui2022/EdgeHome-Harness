param(
    [string]$EvidenceDir = "artifacts\release-demo-smoke",
    [string]$ManifestFileName = "12-evidence-manifest.json",
    [switch]$RequireCleanManifest
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $RepoRoot

try {
    if ([System.IO.Path]::IsPathRooted($EvidenceDir)) {
        $ResolvedEvidenceDir = $EvidenceDir
    }
    else {
        $ResolvedEvidenceDir = Join-Path $RepoRoot $EvidenceDir
    }

    if (-not (Test-Path -LiteralPath $ResolvedEvidenceDir)) {
        throw "evidence directory does not exist: $ResolvedEvidenceDir"
    }

    $manifestPath = Join-Path $ResolvedEvidenceDir $ManifestFileName
    if (-not (Test-Path -LiteralPath $manifestPath)) {
        throw "evidence manifest does not exist: $manifestPath"
    }
    if ((Get-Item -LiteralPath $manifestPath).Length -le 0) {
        throw "evidence manifest is empty: $manifestPath"
    }

    try {
        $manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath | ConvertFrom-Json
    }
    catch {
        throw "evidence manifest is not valid JSON: $($_.Exception.Message)"
    }

    if ($manifest.schema_version -ne "edgehome.demo_evidence_manifest.v1") {
        throw "unsupported evidence manifest schema_version: $($manifest.schema_version)"
    }
    if ($manifest.model_mode -ne "mock") {
        throw "unexpected model_mode in evidence manifest: $($manifest.model_mode)"
    }
    if ($manifest.real_device_execution -ne "disabled_by_default") {
        throw "unexpected real_device_execution in evidence manifest: $($manifest.real_device_execution)"
    }
    if ([string]::IsNullOrWhiteSpace($manifest.git_commit)) {
        throw "evidence manifest is missing git_commit"
    }
    if ([string]::IsNullOrWhiteSpace($manifest.claim_boundary)) {
        throw "evidence manifest is missing claim_boundary"
    }
    if ($RequireCleanManifest -and $manifest.tracked_worktree_dirty) {
        throw "evidence manifest was generated from a dirty tracked worktree"
    }

    $expectedFiles = @(
        "01-release-gate.json",
        "02-ordinary-dry-run.json",
        "03-slot-dry-run.json",
        "04-replay-summary.json",
        "05-trace-frame.json",
        "06-short-memory.json",
        "07-long-memory.json",
        "08-dangerous-blocked.json",
        "09-low-memory-pressure.json",
        "10-backend-readiness.json",
        "11-output-governor-test.txt",
        "public-demo-report.md",
        $ManifestFileName
    )

    foreach ($fileName in $expectedFiles) {
        $path = Join-Path $ResolvedEvidenceDir $fileName
        if (-not (Test-Path -LiteralPath $path)) {
            throw "evidence bundle is missing expected file: $fileName"
        }
        if ((Get-Item -LiteralPath $path).Length -le 0) {
            throw "evidence bundle contains empty file: $fileName"
        }
    }

    $artifacts = @($manifest.artifacts)
    if ($manifest.artifact_count -ne $artifacts.Count) {
        throw "artifact_count $($manifest.artifact_count) does not match artifacts length $($artifacts.Count)"
    }

    $manifestFileSet = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($artifact in $artifacts) {
        if ([string]::IsNullOrWhiteSpace($artifact.file)) {
            throw "evidence manifest contains an artifact without a file name"
        }
        if ([System.IO.Path]::IsPathRooted([string]$artifact.file)) {
            throw "evidence manifest artifact uses an absolute path: $($artifact.file)"
        }
        if (([string]$artifact.file) -match '(^|[\\/])\.\.([\\/]|$)') {
            throw "evidence manifest artifact escapes the evidence directory: $($artifact.file)"
        }
        if (-not $manifestFileSet.Add([string]$artifact.file)) {
            throw "evidence manifest contains duplicate artifact file: $($artifact.file)"
        }
        if ([string]::IsNullOrWhiteSpace($artifact.sha256)) {
            throw "evidence manifest artifact is missing sha256: $($artifact.file)"
        }

        $artifactPath = Join-Path $ResolvedEvidenceDir $artifact.file
        if (-not (Test-Path -LiteralPath $artifactPath)) {
            throw "manifest-listed artifact is missing: $($artifact.file)"
        }

        $item = Get-Item -LiteralPath $artifactPath
        if ($item.Length -le 0) {
            throw "manifest-listed artifact is empty: $($artifact.file)"
        }
        if ($item.Length -ne [int64]$artifact.bytes) {
            throw "byte count mismatch for $($artifact.file): manifest=$($artifact.bytes), actual=$($item.Length)"
        }

        $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $artifactPath).Hash.ToLowerInvariant()
        if ($actualHash -ne ([string]$artifact.sha256).ToLowerInvariant()) {
            throw "sha256 mismatch for $($artifact.file)"
        }
    }

    $missingFromManifest = @(
        $expectedFiles |
            Where-Object { $_ -ne $ManifestFileName -and -not $manifestFileSet.Contains($_) }
    )
    if ($missingFromManifest.Count -gt 0) {
        throw "expected files missing from manifest artifacts: $($missingFromManifest -join ', ')"
    }

    $summary = [ordered]@{
        status = "passed"
        evidence_dir = (Resolve-Path -LiteralPath $ResolvedEvidenceDir).Path
        manifest = $ManifestFileName
        schema_version = $manifest.schema_version
        git_commit = $manifest.git_commit
        tracked_worktree_dirty = [bool]$manifest.tracked_worktree_dirty
        artifact_count = $artifacts.Count
        verified_files = $expectedFiles.Count
    }

    $summary | ConvertTo-Json -Depth 8
}
finally {
    Pop-Location
}
