# Installs the `ward` command (Wardscript) on Windows.
#
#   irm https://raw.githubusercontent.com/ahmedelraei/wardscript/main/install/install.ps1 | iex
#
# Environment:
#   WARD_VERSION          a release tag, like v0.1.0-beta.1 (default: the newest release,
#                         pre-releases included)
#   WARD_INSTALL_DIR      where ward.exe goes (default: %USERPROFILE%\.ward\bin)
#   WARD_NO_MODIFY_PATH   set to 1 to leave the user PATH alone
#   WARD_DOWNLOAD_BASE    serves <tag>/<archive> instead of GitHub releases (for tests)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# Windows PowerShell 5 defaults to TLS 1.0, which GitHub refuses.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Repo = 'ahmedelraei/wardscript'
$InstallDir = if ($env:WARD_INSTALL_DIR) { $env:WARD_INSTALL_DIR } else { Join-Path $env:USERPROFILE '.ward\bin' }

# Windows on Arm runs the x86_64 build under emulation.
$Target = 'x86_64-pc-windows-msvc'
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    Write-Host 'Windows on Arm: installing the x86_64 build, which runs under emulation.'
}

$Version = $env:WARD_VERSION
if (-not $Version) {
    if ($env:WARD_DOWNLOAD_BASE) { throw 'WARD_DOWNLOAD_BASE needs WARD_VERSION' }
    # /releases/latest skips pre-releases, and every release is one during the beta.
    # Only tags like v1.2.3 count: a release made another way may have no archives.
    $Releases = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases?per_page=30" -UseBasicParsing
    $Version = @($Releases | Where-Object { -not $_.draft -and $_.tag_name -match '^v\d' })[0].tag_name
    if (-not $Version) { throw "couldn't find a release of $Repo" }
}
$Base = if ($env:WARD_DOWNLOAD_BASE) { $env:WARD_DOWNLOAD_BASE } else { "https://github.com/$Repo/releases/download" }
$Archive = "ward-$Version-$Target.zip"

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Write-Host "Downloading Wardscript $Version for $Target..."
    $Zip = Join-Path $Tmp $Archive
    Invoke-WebRequest "$Base/$Version/$Archive" -OutFile $Zip -UseBasicParsing
    Invoke-WebRequest "$Base/$Version/$Archive.sha256" -OutFile "$Zip.sha256" -UseBasicParsing

    $Expected = ((Get-Content "$Zip.sha256" -Raw).Trim() -split '\s+')[0].ToLower()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
    if ($Expected -ne $Actual) { throw "checksum mismatch for $Archive (expected $Expected, got $Actual)" }

    $Unpacked = Join-Path $Tmp 'unpacked'
    Expand-Archive $Zip -DestinationPath $Unpacked
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $Exe = Join-Path $InstallDir 'ward.exe'
    # A running ward.exe (say, `ward lsp` in an editor) can't be overwritten, but it can be renamed.
    if (Test-Path $Exe) {
        Remove-Item "$Exe.old" -Force -ErrorAction SilentlyContinue
        Move-Item $Exe "$Exe.old" -Force
    }
    Copy-Item (Join-Path $Unpacked 'ward.exe') $Exe
    Remove-Item "$Exe.old" -Force -ErrorAction SilentlyContinue
    Write-Host "Installed $(& $Exe --version) to $Exe"
}
finally {
    Remove-Item $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$UserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$Entries = @($UserPath -split ';' | Where-Object { $_ })
if ($Entries -notcontains $InstallDir) {
    if ($env:WARD_NO_MODIFY_PATH -eq '1') {
        Write-Host "Add $InstallDir to your PATH to use ``ward``."
    }
    else {
        [Environment]::SetEnvironmentVariable('Path', (($Entries + $InstallDir) -join ';'), 'User')
        $env:Path = "$env:Path;$InstallDir"
        Write-Host "Added $InstallDir to your user PATH. Open a new terminal to use ``ward``."
    }
}

Write-Host ''
Write-Host 'Wardscript is in beta: the language and its diagnostics may change between releases.'
Write-Host 'Get started: ward init hello; cd hello; ward check main.ward'
