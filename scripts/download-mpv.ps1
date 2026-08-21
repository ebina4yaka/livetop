# livetop が使用する libmpv (libmpv-2.dll) を取得するスクリプト
# 入手先: zhongfly/mpv-winbuild の最新リリース (LGPL ビルド)
# 出力: libs/libmpv-2.dll

param(
    [string]$Repo = "zhongfly/mpv-winbuild",
    [string]$OutDir = (Join-Path $PSScriptRoot "..\libs")
)

$ErrorActionPreference = "Stop"

$latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "livetop" }
$asset = $latest.assets | Where-Object { $_.name -like "mpv-dev-lgpl-x86_64-*.7z" } | Select-Object -First 1
if (-not $asset) {
    throw "mpv-dev-lgpl-x86_64 アセットが見つかりません: $($latest.tag_name)"
}

$archive = Join-Path $env:TEMP $asset.name
$extract = Join-Path $env:TEMP ("livetop-mpv-" + [guid]::NewGuid().ToString("N"))

Write-Host "Downloading $($asset.browser_download_url)"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archive

Write-Host "Extracting 7z archive (via bsdtar)..."
New-Item -ItemType Directory -Force -Path $extract | Out-Null
& tar -xf $archive -C $extract
if ($LASTEXITCODE -ne 0) {
    throw "7z 展開に失敗しました。7-Zip をインストールして手動で展開してください: $($asset.browser_download_url)"
}

$dll = Get-ChildItem -Path $extract -Recurse -Filter "libmpv-2.dll" | Select-Object -First 1
if (-not $dll) {
    throw "アーカイブ内に libmpv-2.dll が見つかりません"
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Copy-Item $dll.FullName (Join-Path $OutDir "libmpv-2.dll") -Force

Remove-Item $archive -Force -ErrorAction SilentlyContinue
Remove-Item $extract -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "OK: $(Join-Path $OutDir 'libmpv-2.dll') ($([math]::Round($dll.Length / 1MB, 1)) MB)"
