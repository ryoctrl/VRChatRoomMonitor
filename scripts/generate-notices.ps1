# Generates the list of third-party licenses shown from the app's "License" link.
# Requires: cargo install cargo-about --version 0.9.3 --locked --features cli
# Comments stay in ASCII because Windows PowerShell 5.1 misreads UTF-8 files without a BOM.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

Push-Location $root
try {
    New-Item -ItemType Directory -Force public | Out-Null
    cargo about generate `
        --manifest-path src-tauri/Cargo.toml `
        --config packaging/about.toml `
        --output-file public/third-party-notices.html `
        packaging/about.hbs
    if ($LASTEXITCODE -ne 0) { throw "cargo about failed" }
}
finally {
    Pop-Location
}
