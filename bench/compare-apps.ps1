<#
.SYNOPSIS
  Measures installed mail apps from the outside, all the same way: time from
  launch to a visible window, time until the app goes quiet, memory, number of
  processes, idle CPU and size on disk.

.DESCRIPTION
  Black-box: nothing is read from any app's mail. Each app runs with whatever
  accounts it already has, so mailbox sizes differ between apps; the size of
  each app's data folder is reported to make that visible.

  Launch runs stop the app first (window close, then kill) and start it again,
  so do not run this while a message is being sent. Apps that were running
  before are started again at the end.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File bench\compare-apps.ps1
  powershell -ExecutionPolicy Bypass -File bench\compare-apps.ps1 -Apps Regards,Thunderbird -Runs 3
  powershell -ExecutionPolicy Bypass -File bench\compare-apps.ps1 -Passive
#>
param(
    [string[]]$Apps = @('Regards', 'Spark', 'Mailspring', 'Thunderbird'),
    [int]$Runs = 5,
    [int]$SettleSeconds = 30,
    [int]$IdleSeconds = 30,
    # Only measure apps that are already running; start and stop nothing.
    [switch]$Passive,
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'

# A terminal inside VS Code hands these down, and with them an Electron app
# (Spark, Mailspring) starts as plain Node and exits at once.
Get-ChildItem env: | Where-Object { $_.Name -match '^(ELECTRON_|VSCODE_|NODE_OPTIONS$)' } | ForEach-Object { [Environment]::SetEnvironmentVariable($_.Name, $null) }

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

namespace Bench
{
    public static class Probe
    {
        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        struct PROCESSENTRY32
        {
            public uint dwSize;
            public uint cntUsage;
            public uint th32ProcessID;
            public IntPtr th32DefaultHeapID;
            public uint th32ModuleID;
            public uint cntThreads;
            public uint th32ParentProcessID;
            public int pcPriClassBase;
            public uint dwFlags;
            [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
            public string szExeFile;
        }

        [StructLayout(LayoutKind.Sequential)]
        struct RECT { public int Left, Top, Right, Bottom; }

        delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);

        [DllImport("kernel32.dll")] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, EntryPoint = "Process32FirstW")] static extern bool Process32First(IntPtr h, ref PROCESSENTRY32 e);
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, EntryPoint = "Process32NextW")] static extern bool Process32Next(IntPtr h, ref PROCESSENTRY32 e);
        [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
        [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
        [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr h, out long creation, out long exit, out long kernel, out long user);
        [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
        [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
        [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
        [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT r);

        // Every process named like one of `roots`, and everything they started.
        public static int[] Tree(string[] roots)
        {
            var names = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach (var r in roots) names.Add(r + ".exe");
            var parents = new Dictionary<uint, uint>();
            var found = new HashSet<uint>();
            IntPtr snap = CreateToolhelp32Snapshot(2, 0);
            var e = new PROCESSENTRY32();
            e.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
            if (Process32First(snap, ref e))
            {
                do
                {
                    parents[e.th32ProcessID] = e.th32ParentProcessID;
                    if (names.Contains(e.szExeFile)) found.Add(e.th32ProcessID);
                } while (Process32Next(snap, ref e));
            }
            CloseHandle(snap);
            bool grew = found.Count > 0;
            while (grew)
            {
                grew = false;
                foreach (var kv in parents)
                    if (!found.Contains(kv.Key) && found.Contains(kv.Value)) { found.Add(kv.Key); grew = true; }
            }
            var result = new int[found.Count];
            int i = 0;
            foreach (var p in found) result[i++] = (int)p;
            return result;
        }

        // CPU seconds used so far by the processes now in the tree.
        public static double CpuSeconds(string[] roots)
        {
            long total = 0;
            foreach (int pid in Tree(roots))
            {
                IntPtr h = OpenProcess(0x1000, false, (uint)pid);
                if (h == IntPtr.Zero) continue;
                long c, x, k, u;
                if (GetProcessTimes(h, out c, out x, out k, out u)) total += k + u;
                CloseHandle(h);
            }
            return total / 1e7;
        }

        public static bool HasWindow(string[] roots)
        {
            var pids = new HashSet<uint>();
            foreach (int p in Tree(roots)) pids.Add((uint)p);
            if (pids.Count == 0) return false;
            bool seen = false;
            EnumWindows(delegate (IntPtr hwnd, IntPtr l)
            {
                if (!IsWindowVisible(hwnd)) return true;
                uint pid;
                GetWindowThreadProcessId(hwnd, out pid);
                if (!pids.Contains(pid)) return true;
                RECT r;
                if (!GetWindowRect(hwnd, out r)) return true;
                if (r.Right - r.Left >= 300 && r.Bottom - r.Top >= 200) { seen = true; return false; }
                return true;
            }, IntPtr.Zero);
            return seen;
        }

        // Starts the app. Returns { ms to a visible window, ms until quiet, CPU
        // seconds used until quiet }; -1 where the timeout came first. Quiet:
        // one second in a row under 10% of one core.
        public static double[] Launch(string exe, string args, string[] roots, int timeoutMs)
        {
            var psi = new ProcessStartInfo(exe, args);
            psi.UseShellExecute = true;
            var clock = Stopwatch.StartNew();
            Process.Start(psi);
            double window = -1, quiet = -1, cpuAtQuiet = -1;
            while (clock.ElapsedMilliseconds < timeoutMs)
            {
                if (HasWindow(roots)) { window = clock.Elapsed.TotalMilliseconds; break; }
                Thread.Sleep(5);
            }
            if (window < 0) return new double[] { -1, -1, -1 };
            int calm = 0;
            double streakStart = 0;
            double lastCpu = CpuSeconds(roots);
            double lastAt = clock.Elapsed.TotalSeconds;
            while (clock.ElapsedMilliseconds < timeoutMs)
            {
                Thread.Sleep(250);
                double cpu = CpuSeconds(roots);
                double at = clock.Elapsed.TotalSeconds;
                double load = (cpu - lastCpu) / (at - lastAt);
                if (load < 0.10)
                {
                    if (calm == 0) streakStart = lastAt * 1000;
                    calm++;
                    if (calm >= 4) { quiet = streakStart; cpuAtQuiet = cpu; break; }
                }
                else calm = 0;
                lastCpu = cpu;
                lastAt = at;
            }
            return new double[] { window, quiet, cpuAtQuiet };
        }
    }
}
'@

function Get-FolderMB([string[]]$Paths, [string[]]$Exclude = @()) {
    $total = 0
    foreach ($p in $Paths) {
        if (-not (Test-Path -LiteralPath $p)) { continue }
        Get-ChildItem -LiteralPath $p -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
            $skip = $false
            foreach ($x in $Exclude) { if ($_.FullName -like $x) { $skip = $true } }
            if (-not $skip) { $total += $_.Length }
        }
    }
    [math]::Round($total / 1MB, 1)
}

$local = $env:LOCALAPPDATA
$roaming = $env:APPDATA
$regardsDir = Join-Path $local 'Regards'

# Roots: process names that belong to the app; everything they start (helper
# processes, the system WebView2 that Regards uses) is counted with them.
$catalog = [ordered]@{
    Regards     = @{
        Exe = Join-Path $regardsDir 'regards-desktop.exe'; Args = ''
        Roots = @('regards-desktop', 'regards')
        Install = { Get-FolderMB (Get-ChildItem $regardsDir -Filter *.exe).FullName }
        Data = { Get-FolderMB $regardsDir -Exclude '*.exe' }
        PreStop = { & (Join-Path $regardsDir 'regards.exe') daemon stop 2>$null | Out-Null }
    }
    Spark       = @{
        Exe = Join-Path $local 'Programs\SparkDesktop\Spark Desktop.exe'; Args = ''
        Roots = @('Spark Desktop')
        Install = { Get-FolderMB (Join-Path $local 'Programs\SparkDesktop') }
        Data = { Get-FolderMB (Join-Path $roaming 'Spark Desktop'), (Join-Path $local 'Spark Desktop') }
    }
    Mailspring  = @{
        Exe = Join-Path $local 'Mailspring\mailspring.exe'; Args = ''
        Roots = @('mailspring')
        Install = { Get-FolderMB (Join-Path $local 'Mailspring') }
        Data = { Get-FolderMB (Join-Path $roaming 'Mailspring') }
    }
    Thunderbird = @{
        Exe = 'C:\Program Files\Mozilla Thunderbird\thunderbird.exe'; Args = ''
        Roots = @('thunderbird')
        Install = { Get-FolderMB 'C:\Program Files\Mozilla Thunderbird' }
        Data = { Get-FolderMB (Join-Path $roaming 'Thunderbird'), (Join-Path $local 'Thunderbird') }
    }
    Outlook     = @{
        Exe = 'explorer.exe'; Args = 'shell:AppsFolder\Microsoft.OutlookForWindows_8wekyb3d8bbwe!Microsoft.OutlookforWindows'
        Roots = @('olk')
        Install = { Get-FolderMB (Get-AppxPackage Microsoft.OutlookForWindows).InstallLocation }
        Data = { Get-FolderMB (Join-Path $local 'Microsoft\Olk') }
    }
}

function Test-Running($app) { ([Bench.Probe]::Tree($app.Roots)).Count -gt 0 }

function Wait-Gone($app, [int]$Seconds) {
    $until = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $until) {
        if (-not (Test-Running $app)) { return $true }
        Start-Sleep -Milliseconds 200
    }
    -not (Test-Running $app)
}

function Stop-App($app) {
    if (-not (Test-Running $app)) { return }
    if ($app.PreStop) { try { & $app.PreStop } catch {} }
    foreach ($id in [Bench.Probe]::Tree($app.Roots)) {
        try { (Get-Process -Id $id -ErrorAction Stop).CloseMainWindow() | Out-Null } catch {}
    }
    if (Wait-Gone $app 4) { return }
    # Tray apps only hide on close.
    foreach ($id in [Bench.Probe]::Tree($app.Roots)) {
        try { Stop-Process -Id $id -Force -ErrorAction Stop } catch {}
    }
    if (-not (Wait-Gone $app 10)) { throw "could not stop $($app.Roots -join ', ')" }
}

function Get-Memory($app) {
    $ids = [Bench.Probe]::Tree($app.Roots)
    $private = 0; $commit = 0
    if ($ids.Count -gt 0) {
        $perf = Get-CimInstance Win32_PerfRawData_PerfProc_Process | Where-Object { $ids -contains [int]$_.IDProcess }
        foreach ($p in $perf) { $private += $p.WorkingSetPrivate; $commit += $p.PrivateBytes }
    }
    @{ Processes = $ids.Count; PrivateWorkingSetMB = [math]::Round($private / 1MB); CommitMB = [math]::Round($commit / 1MB) }
}

function Get-IdleCpuPercent($app, [int]$Seconds) {
    $before = [Bench.Probe]::CpuSeconds($app.Roots)
    Start-Sleep -Seconds $Seconds
    $after = [Bench.Probe]::CpuSeconds($app.Roots)
    # Percent of one core.
    [math]::Round(100 * ($after - $before) / $Seconds, 2)
}

function Get-Median([double[]]$Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return $null }
    $mid = [int][math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2) { $sorted[$mid] } else { ($sorted[$mid - 1] + $sorted[$mid]) / 2 }
}

$results = @()
foreach ($name in $Apps) {
    $app = $catalog[$name]
    if (-not $app) { Write-Warning "unknown app $name"; continue }
    if ($app.Exe -ne 'explorer.exe' -and -not (Test-Path -LiteralPath $app.Exe)) { Write-Warning "$name is not installed"; continue }

    $wasRunning = Test-Running $app
    $background = $false
    if ($wasRunning) {
        $ids = [Bench.Probe]::Tree($app.Roots)
        $background = [bool](Get-CimInstance Win32_Process | Where-Object { $ids -contains [int]$_.ProcessId -and $_.CommandLine -match '--background' })
    }
    $version = if ($app.Exe -ne 'explorer.exe') { (Get-Item -LiteralPath $app.Exe).VersionInfo.ProductVersion } else { (Get-AppxPackage Microsoft.OutlookForWindows).Version }
    $row = [ordered]@{
        App = $name; Version = $version
        InstallMB = & $app.Install; DataMB = & $app.Data
        WindowMs = @(); QuietMs = @(); StartCpuS = @()
    }

    if ($Passive) {
        if (-not $wasRunning) { Write-Host "$name is not running, skipped (passive)"; continue }
    }
    else {
        for ($i = 1; $i -le $Runs; $i++) {
            Stop-App $app
            Start-Sleep -Seconds 2
            $t = [Bench.Probe]::Launch($app.Exe, $app.Args, $app.Roots, 60000)
            Write-Host ("{0} run {1}: window {2:n0} ms, quiet {3:n0} ms, cpu {4:n2} s" -f $name, $i, $t[0], $t[1], $t[2])
            if ($t[0] -ge 0) { $row.WindowMs += $t[0] }
            if ($t[1] -ge 0) { $row.QuietMs += $t[1]; $row.StartCpuS += $t[2] }
        }
        Start-Sleep -Seconds $SettleSeconds
    }

    $memory = Get-Memory $app
    $row.Processes = $memory.Processes
    $row.PrivateWorkingSetMB = $memory.PrivateWorkingSetMB
    $row.CommitMB = $memory.CommitMB
    $row.IdleCpuPercent = Get-IdleCpuPercent $app $IdleSeconds
    $row.WindowMsMedian = Get-Median $row.WindowMs
    $row.QuietMsMedian = Get-Median $row.QuietMs
    $row.StartCpuSMedian = Get-Median $row.StartCpuS
    $results += [pscustomobject]$row

    if (-not $Passive) {
        Stop-App $app
        if ($wasRunning) {
            $arguments = if ($background) { '--background' } else { $app.Args }
            if ($arguments) { Start-Process -FilePath $app.Exe -ArgumentList $arguments } else { Start-Process -FilePath $app.Exe }
        }
    }
}

$machine = [ordered]@{
    Date = (Get-Date -Format 'yyyy-MM-dd HH:mm')
    Cpu = (Get-CimInstance Win32_Processor).Name.Trim()
    RamGB = [int]((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
    Windows = (Get-CimInstance Win32_OperatingSystem).Version
    Runs = $Runs; Passive = [bool]$Passive
}

''
$results | Format-Table App, Version,
    @{ n = 'Window ms'; e = { if ($null -ne $_.WindowMsMedian) { [int]$_.WindowMsMedian } } },
    @{ n = 'Quiet ms'; e = { if ($null -ne $_.QuietMsMedian) { [int]$_.QuietMsMedian } } },
    @{ n = 'Start CPU s'; e = { if ($null -ne $_.StartCpuSMedian) { [math]::Round($_.StartCpuSMedian, 2) } } },
    @{ n = 'Procs'; e = { $_.Processes } },
    @{ n = 'RAM MB'; e = { $_.PrivateWorkingSetMB } },
    @{ n = 'Commit MB'; e = { $_.CommitMB } },
    @{ n = 'Idle CPU %'; e = { $_.IdleCpuPercent } },
    @{ n = 'Install MB'; e = { $_.InstallMB } },
    @{ n = 'Data MB'; e = { $_.DataMB } } -AutoSize

if ($Out) {
    [ordered]@{ Machine = $machine; Results = $results } | ConvertTo-Json -Depth 5 | Out-File -FilePath $Out -Encoding utf8
    "Saved $Out"
}
