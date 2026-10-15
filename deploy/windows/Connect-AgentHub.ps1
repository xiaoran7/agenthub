[CmdletBinding()]
param(
    [string]$ServerUrl = "",
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

if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
    $ServerUrl = Read-Host "请输入自部署 AgentHub 服务器地址（例如 http://localhost:3000 或 http://你的服务器IP:3000）"
}

$ServerUrl = $ServerUrl.Trim().TrimEnd("/")
if ([string]::IsNullOrWhiteSpace($ServerUrl)) {
    throw "服务器地址不能为空。"
}
$parsedUrl = $null
if (-not [Uri]::TryCreate($ServerUrl, [UriKind]::Absolute, [ref]$parsedUrl)) {
    throw "服务器地址不是有效的 URL。"
}
if ($parsedUrl.Scheme -ne "https" -and $parsedUrl.Scheme -ne "http") {
    throw "服务器地址必须使用 http 或 https 协议。"
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
$connectionMarkerPath = Join-Path $stateDirectory "connected-server.txt"
New-Item -ItemType Directory -Force -Path $stateDirectory | Out-Null

Write-Step "准备设备身份"
$identityCreated = $false
if (-not (Test-Path -LiteralPath $identityPath -PathType Leaf)) {
    Invoke-Runner @("device", "init", "--identity", $identityPath)
    $identityCreated = $true
}

$connectedServer = if (Test-Path -LiteralPath $connectionMarkerPath -PathType Leaf) {
    (Get-Content -Raw -LiteralPath $connectionMarkerPath).Trim()
} else {
    ""
}

if ($identityCreated -or $connectedServer -ne $ServerUrl) {
    if ([string]::IsNullOrWhiteSpace($BindingCode)) {
        $BindingCode = Read-Host "请粘贴控制台中的一次性设备连接码（10 分钟内有效）"
    }
    if ([string]::IsNullOrWhiteSpace($BindingCode)) {
        throw "连接码不能为空。"
    }

    Write-Step "连接到服务器"
    Invoke-Runner @(
        "device", "bind",
        "--identity", $identityPath,
        "--server", $ServerUrl,
        "--binding-code", $BindingCode.Trim()
    )
    Set-Content -LiteralPath $connectionMarkerPath -Value $ServerUrl -Encoding utf8
}

Write-Host "`n设备已连接：$ServerUrl" -ForegroundColor Green
Write-Host "Runner 正在运行。请保持此窗口开启；按 Ctrl+C 可停止。" -ForegroundColor Yellow
Invoke-Runner @(
    "--identity", $identityPath,
    "run", "all",
    "--db", $databasePath,
    "--sync-server", $ServerUrl
)
