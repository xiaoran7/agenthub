[CmdletBinding()]
param(
    [string]$PlatformUrl = "https://platform.tannerlab.xyz",
    [string]$BindingCode = ""
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host "`n$Message" -ForegroundColor Cyan
}

function Invoke-Runner([string[]]$Arguments) {
    & $script:RunnerPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Runner 执行失败（退出码 $LASTEXITCODE）。请确认接入包完整后重试。"
    }
}

if ([string]::IsNullOrWhiteSpace($PlatformUrl)) {
    $PlatformUrl = Read-Host "平台地址（直接回车使用 https://platform.tannerlab.xyz）"
    if ([string]::IsNullOrWhiteSpace($PlatformUrl)) {
        $PlatformUrl = "https://platform.tannerlab.xyz"
    }
}

$PlatformUrl = $PlatformUrl.Trim().TrimEnd("/")
$parsedUrl = $null
if (-not [Uri]::TryCreate($PlatformUrl, [UriKind]::Absolute, [ref]$parsedUrl)) {
    throw "平台地址不是有效的 URL。"
}
if ($parsedUrl.Scheme -ne "https" -and $parsedUrl.Host -notin @("localhost", "127.0.0.1", "[::1]")) {
    throw "远程平台必须使用 HTTPS。"
}

$script:RunnerPath = Join-Path $PSScriptRoot "agenthub-runner.exe"
if (-not (Test-Path -LiteralPath $script:RunnerPath -PathType Leaf)) {
    $script:RunnerPath = Join-Path $PSScriptRoot "agent-pulse-runner.exe"
}
if (-not (Test-Path -LiteralPath $script:RunnerPath -PathType Leaf)) {
    throw "找不到 agenthub-runner.exe。请把 Runner 程序放到本脚本旁边后重试。"
}

$stateDirectory = Join-Path $env:LOCALAPPDATA "AgentHub"
$identityPath = Join-Path $stateDirectory "runner-identity.json"
$databasePath = Join-Path $stateDirectory "runner.db"
$connectionMarkerPath = Join-Path $stateDirectory "connected-platform.txt"
New-Item -ItemType Directory -Force -Path $stateDirectory | Out-Null

Write-Step "准备设备身份"
$identityCreated = $false
if (-not (Test-Path -LiteralPath $identityPath -PathType Leaf)) {
    Invoke-Runner @("device", "init", "--identity", $identityPath)
    $identityCreated = $true
}

$connectedPlatform = if (Test-Path -LiteralPath $connectionMarkerPath -PathType Leaf) {
    (Get-Content -Raw -LiteralPath $connectionMarkerPath).Trim()
} else {
    ""
}

if ($identityCreated -or $connectedPlatform -ne $PlatformUrl) {
    if ([string]::IsNullOrWhiteSpace($BindingCode)) {
        $BindingCode = Read-Host "请粘贴平台中的一次性连接码（10 分钟内有效）"
    }
    if ([string]::IsNullOrWhiteSpace($BindingCode)) {
        throw "连接码不能为空。"
    }

    Write-Step "连接到平台"
    Invoke-Runner @(
        "device", "bind",
        "--identity", $identityPath,
        "--server", $PlatformUrl,
        "--binding-code", $BindingCode.Trim()
    )
    Set-Content -LiteralPath $connectionMarkerPath -Value $PlatformUrl -Encoding utf8
}

Write-Host "`n设备已连接：$PlatformUrl" -ForegroundColor Green
Write-Host "Runner 正在运行。请保持此窗口开启；按 Ctrl+C 可停止。" -ForegroundColor Yellow
Invoke-Runner @(
    "--identity", $identityPath,
    "run", "all",
    "--db", $databasePath,
    "--sync-server", $PlatformUrl
)
