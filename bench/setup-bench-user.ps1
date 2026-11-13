<#
.SYNOPSIS
  Run once, signed in as the Windows user made for the benchmark: installs the
  mail apps that are installed per user, in their latest released version.
  Needs no administrator rights. See bench/README.md.
#>
$ErrorActionPreference = 'Stop'

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Warning 'winget is not ready for this user yet. Open the Microsoft Store once, wait a minute, and run this again.'
    exit 1
}

foreach ($id in 'Foundry376.Mailspring', 'Readdle.Spark') {
    Write-Host "Installing $id"
    winget install --exact --id $id --source winget --accept-package-agreements --accept-source-agreements
}

# Regards: the latest release's Windows installer, from GitHub.
$release = Invoke-RestMethod 'https://api.github.com/repos/lucienPochon/regards/releases/latest'
$asset = $release.assets | Where-Object { $_.name -like '*_x64-setup.exe' } | Select-Object -First 1
if (-not $asset) { throw "no Windows installer in release $($release.tag_name)" }
$installer = Join-Path $env:TEMP $asset.name
Write-Host "Installing Regards $($release.tag_name)"
Invoke-WebRequest $asset.browser_download_url -OutFile $installer
Start-Process -FilePath $installer -ArgumentList '/S' -Wait

''
'Installed for this user:'
foreach ($path in "$env:LOCALAPPDATA\Regards\regards-desktop.exe", "$env:LOCALAPPDATA\Programs\SparkDesktop\Spark Desktop.exe", "$env:LOCALAPPDATA\Mailspring\mailspring.exe", 'C:\Program Files\Mozilla Thunderbird\thunderbird.exe') {
    $state = if (Test-Path -LiteralPath $path) { (Get-Item -LiteralPath $path).VersionInfo.ProductVersion } else { 'MISSING' }
    '  {0}  {1}' -f $path, $state
}
''
'Thunderbird is installed for the whole computer: update it from an administrator account if it is not the latest.'
'Next: open each app, add the benchmark mailbox and nothing else, and leave it open until it has fetched all the mail.'
'Then: powershell -ExecutionPolicy Bypass -File compare-apps.ps1 -Out results.json'
