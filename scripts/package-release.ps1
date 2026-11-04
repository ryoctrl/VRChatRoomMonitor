# Collects the output of `pnpm tauri build` into artifacts/ as the files to publish.
# Comments stay in ASCII because Windows PowerShell 5.1 misreads UTF-8 files without a BOM.
param(
    [string]$TargetDir = "target"
)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    $version = (Select-String -Path src-tauri/Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
    $exe = Join-Path $TargetDir "release/VRChatRoomMonitor.exe"
    $installer = Get-ChildItem (Join-Path $TargetDir "release/bundle/nsis") -Filter "*_$($version)_*-setup.exe" | Select-Object -First 1
    if (-not (Test-Path $exe)) { throw "exe not found: $exe" }
    if (-not $installer) { throw "installer not found" }

    $artifacts = Join-Path $root "artifacts"
    New-Item -ItemType Directory -Force $artifacts | Out-Null
    Get-ChildItem $artifacts | Remove-Item -Recurse -Force
    Copy-Item $exe $artifacts
    Copy-Item $installer.FullName (Join-Path $artifacts "VRChatRoomMonitor-$version-setup.exe")

    # The readme uses UTF-8 with a BOM and CRLF so that every text editor shows it correctly.
    $folder = Join-Path $artifacts "VRChatRoomMonitor-$version"
    New-Item -ItemType Directory $folder | Out-Null
    $readme = (Get-Content packaging/README.txt -Raw -Encoding UTF8).Replace("{VERSION}", $version) -replace "\r?\n", "`r`n"
    [IO.File]::WriteAllText((Join-Path $folder "README.txt"), $readme, [Text.UTF8Encoding]::new($true))
    Copy-Item $exe $folder
    Copy-Item LICENSE (Join-Path $folder "LICENSE.txt")
    Copy-Item public/third-party-notices.html (Join-Path $folder "THIRD-PARTY-NOTICES.html")

    $zip = Join-Path $artifacts "VRChatRoomMonitor-$version-windows-x64.zip"
    Compress-Archive -Path $folder -DestinationPath $zip
    Remove-Item -Recurse -Force $folder
    Get-ChildItem $artifacts | ForEach-Object { "{0,10} {1}" -f $_.Length, $_.Name }
}
finally {
    Pop-Location
}
