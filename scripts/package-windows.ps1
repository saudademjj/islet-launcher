# Builds the Tauri app on Windows: an NSIS installer plus a portable exe.
# Needs Rust (MSVC toolchain), Visual Studio Build Tools (C++ + Windows SDK)
# and Node.js 20+. The WebView2 runtime ships with Windows 11.
#
# Code signing (optional; unsigned builds trigger SmartScreen warnings):
#   WINDOWS_SIGN_CERT      path to a .pfx certificate
#   WINDOWS_SIGN_PASSWORD  its password
#   or WINDOWS_SIGN_THUMBPRINT for a certificate already in the store.
#   WINDOWS_SIGN_TIMESTAMP overrides the timestamp server.
# Tauri signs the app and the installer through bundle.windows.signCommand.
#
# With RELEASE_BASE_URL and RELEASE_PAGE set, the installer is added to
# build\latest.json (scripts\update-manifest.py) for the update check.
#
# -Quick builds a package to try out rather than ship: thin LTO over 16
# codegen units links in a fraction of the time for a slightly larger exe.
# Switching between quick and full builds recompiles every dependency.
param([string]$ClientId = "", [switch]$SkipTests, [switch]$Quick)
$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path $PSScriptRoot -Parent
Push-Location $ProjectRoot
try {
    if ($ClientId) { $env:MC_LAUNCHER_CLIENT_ID = $ClientId }
    if (-not $env:MC_LAUNCHER_CLIENT_ID) {
        Write-Warning "MC_LAUNCHER_CLIENT_ID is not set; players will be asked for their own Azure Client ID before Microsoft sign-in."
    }
    foreach ($Tool in "cargo", "node", "npm") {
        if (-not (Get-Command $Tool -ErrorAction SilentlyContinue)) { throw "Missing $Tool. Install Rust (MSVC) from https://rustup.rs/ and Node.js 20+." }
    }
    # Added only once: flags that differ from the last build, even by a
    # repeat, make cargo rebuild every dependency.
    if ($env:RUSTFLAGS -notmatch 'crt-static') { $env:RUSTFLAGS = "$env:RUSTFLAGS -C target-feature=+crt-static".Trim() }
    # Keep this machine's paths (user name, cargo registry, checkout) out of
    # the exe's panic messages. RUSTFLAGS splits on spaces, so a path with
    # one is left as it is.
    $CargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
    foreach ($Map in "$CargoHome=/cargo", "$ProjectRoot=/islet") {
        $Flag = "--remap-path-prefix=$Map"
        if ($Map -notmatch ' ' -and -not $env:RUSTFLAGS.Contains($Flag)) { $env:RUSTFLAGS = "$env:RUSTFLAGS $Flag".Trim() }
    }
    # Every core by default; JOBS=4 keeps the machine responsive meanwhile.
    if ($env:JOBS) { $env:CARGO_BUILD_JOBS = $env:JOBS }
    if ($Quick) {
        $env:CARGO_PROFILE_RELEASE_LTO = "thin"
        $env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "16"
        Write-Host "==> quick build (thin LTO), not for release"
    }

    if (-not (Test-Path node_modules)) {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed." }
    }
    if (-not $SkipTests) {
        # All targets: the examples call the core API too and must keep compiling.
        cargo test --locked -p islet --all-targets
        if ($LASTEXITCODE -ne 0) { throw "Core tests failed." }
        npm test
        if ($LASTEXITCODE -ne 0) { throw "UI tests failed." }
    }
    $SignArgs = $null
    $Timestamp = if ($env:WINDOWS_SIGN_TIMESTAMP) { $env:WINDOWS_SIGN_TIMESTAMP } else { "http://timestamp.digicert.com" }
    if ($env:WINDOWS_SIGN_CERT) {
        if (-not (Test-Path $env:WINDOWS_SIGN_CERT)) { throw "WINDOWS_SIGN_CERT not found: $env:WINDOWS_SIGN_CERT" }
        $SignArgs = @("/f", (Resolve-Path $env:WINDOWS_SIGN_CERT).Path)
        if ($env:WINDOWS_SIGN_PASSWORD) { $SignArgs += @("/p", $env:WINDOWS_SIGN_PASSWORD) }
    } elseif ($env:WINDOWS_SIGN_THUMBPRINT) {
        $SignArgs = @("/sha1", $env:WINDOWS_SIGN_THUMBPRINT)
    }
    $Bundle = @("--bundles", "nsis")
    if ($SignArgs) {
        $SignTool = Get-Command signtool.exe -ErrorAction SilentlyContinue
        if (-not $SignTool) {
            $SignTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
        }
        if (-not $SignTool) { throw "signtool.exe not found; install the Windows SDK." }
        $SignTool = if ($SignTool.Source) { $SignTool.Source } else { $SignTool.FullName }
        # Tauri runs this for the app exe and the installer; %1 is the file.
        $Command = @{ cmd = $SignTool; args = @("sign", "/fd", "sha256", "/tr", $Timestamp, "/td", "sha256") + $SignArgs + @("%1") }
        $Config = @{ bundle = @{ windows = @{ signCommand = $Command } } } | ConvertTo-Json -Depth 6 -Compress
        $ConfigFile = Join-Path $env:TEMP "islet-sign.json"
        # No byte-order mark: Tauri's JSON reader rejects one.
        [IO.File]::WriteAllText($ConfigFile, $Config)
        $Bundle += @("--config", $ConfigFile)
        Write-Host "==> signing with signtool"
    } else {
        Write-Host "==> unsigned build (set WINDOWS_SIGN_CERT or WINDOWS_SIGN_THUMBPRINT to sign)"
    }
    try {
        npm run tauri build -- @Bundle
        if ($LASTEXITCODE -ne 0) { throw "Windows build failed." }
    } finally {
        if ($ConfigFile) { Remove-Item $ConfigFile -ErrorAction SilentlyContinue }
    }

    $Version = (Select-String -Path src-tauri\Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    # The machine's own architecture: the build is native, not cross-compiled.
    $Arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
    $Platform = if ($Arch -eq "arm64") { "windows-aarch64" } else { "windows-x86_64" }
    $Out = Join-Path $ProjectRoot "build\windows"
    New-Item -ItemType Directory -Force $Out | Out-Null
    Remove-Item (Join-Path $Out "*.exe") -ErrorAction SilentlyContinue
    # Plain names: the updater only downloads names without spaces.
    # The newest one: an earlier build under another name may still be there.
    $Setup = Get-ChildItem "target\release\bundle\nsis\*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $Setup) { throw "No installer was built." }
    Copy-Item $Setup.FullName (Join-Path $Out "Islet-$Version-$Arch-setup.exe") -Force
    Copy-Item "target\release\islet.exe" (Join-Path $Out "Islet-$Version-$Arch-portable.exe") -Force
    if ($SignArgs) {
        foreach ($File in Get-ChildItem $Out -Filter *.exe) {
            # Tauri signs the exe it bundles; sign anything it left alone.
            if (-not (Get-AuthenticodeSignature $File.FullName).SignerCertificate) {
                & $SignTool sign /fd sha256 /tr $Timestamp /td sha256 @SignArgs $File.FullName
                if ($LASTEXITCODE -ne 0) { throw "Signing $($File.Name) failed." }
            }
        }
    }
    if ($env:RELEASE_BASE_URL -and $env:RELEASE_PAGE) {
        $Installer = Join-Path $Out "Islet-$Version-$Arch-setup.exe"
        python scripts\update-manifest.py build\latest.json --version $Version --page $env:RELEASE_PAGE --base-url $env:RELEASE_BASE_URL --notes "$env:RELEASE_NOTES" "$Platform=$Installer"
        if ($LASTEXITCODE -ne 0) { throw "Update manifest failed." }
    }
    Get-ChildItem $Out | Format-Table Name, Length
} finally {
    Pop-Location
}
