# Runs the full local check suite. Stops on the first failure.
# Failure is detected via exit codes: cargo writes progress to stderr, which
# Windows PowerShell 5.1 would treat as an error under ErrorActionPreference=Stop.
Set-Location (Join-Path $PSScriptRoot '..')

$steps = @(
    @('fmt', 'cargo', @('fmt', '--all', '--check')),
    @('clippy', 'cargo', @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')),
    @('test', 'cargo', @('test', '--workspace')),
    @('release build', 'cargo', @('build', '--release', '--workspace'))
)
foreach ($step in $steps) {
    Write-Host "==> $($step[0])"
    & $step[1] @($step[2])
    if ($LASTEXITCODE -ne 0) { throw "$($step[0]) failed with exit code $LASTEXITCODE" }
}
Write-Host 'All checks passed.'
