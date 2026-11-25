[CmdletBinding()]
param(
    [string]$ServerUrl = "",
    [string]$BindingCode = "",
    [string]$Workspace = "",
    [string]$CodexBinary = "codex",
    [int]$WebPort = 4200
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host "`n$Message" -ForegroundColor Cyan
}

function ConvertFrom-SecureStringPlain([Security.SecureString]$SecureValue) {
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($SecureValue)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    } finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    }
}

function Invoke-Runner([string[]]$Arguments) {
    & $script:RunnerPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Runner exited with code $LASTEXITCODE. Verify the bundle files and try again."
    }
}

if ($WebPort -lt 1 -or $WebPort -gt 65535) {
    throw "WebPort must be between 1 and 65535."
}

if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
    $ServerUrl = Read-Host "Enter your AgentHub Gateway URL (e.g. https://agent.example.com or http://localhost:3000)"
}

$ServerUrl = $ServerUrl.Trim().TrimEnd("/")
if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
    throw "Server URL cannot be empty."
}
$parsedUrl = $null
if (-not [Uri]::TryCreate($ServerUrl, [UriKind]::Absolute, [ref]$parsedUrl)) {
    throw "Server URL is not a valid absolute URL."
}
if ($parsedUrl.Scheme -ne "https" -and $parsedUrl.Scheme -ne "http") {
    throw "Server URL must use the http or https scheme."
}

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = (Get-Location).Path
    if ([string]::IsNullOrWhiteSpace($Workspace)) {
        $Workspace = $PSScriptRoot
    }
}
$Workspace = [IO.Path]::GetFullPath($Workspace.Trim())
if (-not (Test-Path -LiteralPath $Workspace -PathType Container)) {
    throw "Workspace directory does not exist: $Workspace"
}

$script:RunnerPath = Join-Path $PSScriptRoot "agenthub-runner.exe"
if (-not (Test-Path -LiteralPath $script:RunnerPath -PathType Leaf)) {
    $script:RunnerPath = Join-Path $PSScriptRoot "agent-pulse-runner.exe"
}
if (-not (Test-Path -LiteralPath $script:RunnerPath -PathType Leaf)) {
    throw "Cannot find agenthub-runner.exe next to this script."
}

$stateDirectory = Join-Path $env:LOCALAPPDATA "AgentHub"
$identityPath = Join-Path $stateDirectory "runner-identity.json"
$databasePath = Join-Path $stateDirectory "runner.db"
$connectionMarkerPath = Join-Path $stateDirectory "connected-server.txt"
New-Item -ItemType Directory -Force -Path $stateDirectory | Out-Null

Write-Step "Preparing device identity"
$identityCreated = $false
if (-not (Test-Path -LiteralPath $identityPath -PathType Leaf)) {
    Invoke-Runner @("device", "init", "--identity", $identityPath)
    $identityCreated = $true
}

$connectedServer = if (Test-Path -LiteralPath $connectionMarkerPath -PathType Leaf) {
    $rawMarker = Get-Content -Raw -LiteralPath $connectionMarkerPath
    if ([string]::IsNullOrWhiteSpace($rawMarker)) { "" } else { $rawMarker.Trim() }
} else {
    ""
}

if ($identityCreated -or $connectedServer -ne $ServerUrl) {
    $resolvedCode = ""
    if ([string]::IsNullOrWhiteSpace($BindingCode)) {
        $secureCode = Read-Host "Paste the one-time device binding code from the console (valid for 10 minutes)" -AsSecureString
        $resolvedCode = (ConvertFrom-SecureStringPlain $secureCode).Trim()
    } else {
        $resolvedCode = $BindingCode.Trim()
    }
    if ([string]::IsNullOrWhiteSpace($resolvedCode)) {
        throw "Binding code cannot be empty."
    }

    Write-Step "Binding device to Gateway"
    $env:AGENT_PLATFORM_BINDING_CODE = $resolvedCode
    try {
        Invoke-Runner @(
            "device", "bind",
            "--identity", $identityPath,
            "--server", $ServerUrl
        )
    } finally {
        Remove-Item Env:AGENT_PLATFORM_BINDING_CODE -ErrorAction SilentlyContinue
        $resolvedCode = ""
        $BindingCode = ""
    }
    Set-Content -LiteralPath $connectionMarkerPath -Value $ServerUrl -Encoding utf8
}

Write-Host "`nDevice bound to: $ServerUrl" -ForegroundColor Green
Write-Host "Workspace: $Workspace" -ForegroundColor Gray
Write-Host "Local console: http://127.0.0.1:$WebPort" -ForegroundColor Gray
Write-Host "Runner is starting in Remote v3 mode. Keep this window open or press Ctrl+C to stop." -ForegroundColor Yellow

Invoke-Runner @(
    "--identity", $identityPath,
    "remote-run",
    "--codex-binary", $CodexBinary,
    "--workspace", $Workspace,
    "--db", $databasePath,
    "--server", $ServerUrl,
    "--web-port", "$WebPort",
    "--open-browser", "false"
)
