<#
.SYNOPSIS
  Measures installed mail apps from the outside, all the same way: time from
  launch to a visible window, to its content being drawn and to the app going
  quiet, then memory, number of processes, idle CPU and size on disk.
  The rules for numbers that are published are in bench/README.md.

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
    # Launches that count. One more is made first and thrown away.
    [int]$Runs = 10,
    [int]$SettleSeconds = 60,
    [int]$IdleSeconds = 60,
    # Only measure apps that are already running; start and stop nothing.
    [switch]$Passive,
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'

# A terminal inside VS Code hands these down, and with them an Electron app
# (Spark, Mailspring) starts as plain Node and exits at once.
Get-ChildItem env: | Where-Object { $_.Name -match '^(ELECTRON_|VSCODE_|NODE_OPTIONS$)' } | ForEach-Object { [Environment]::SetEnvironmentVariable($_.Name, $null) }

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Drawing.Imaging;
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

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr h, uint flags, System.Text.StringBuilder path, ref uint size);

        // Only programs under this folder count as the app: another copy
        // elsewhere (a development build, a beta) is left alone.
        public static string Folder = "";

        static string PathOf(uint pid)
        {
            IntPtr h = OpenProcess(0x1000, false, pid);
            if (h == IntPtr.Zero) return "";
            var path = new System.Text.StringBuilder(1024);
            uint size = 1024;
            bool ok = QueryFullProcessImageName(h, 0, path, ref size);
            CloseHandle(h);
            return ok ? path.ToString() : "";
        }

        static long StartedAt(uint pid)
        {
            IntPtr h = OpenProcess(0x1000, false, pid);
            if (h == IntPtr.Zero) return 0;
            long c, x, k, u;
            bool ok = GetProcessTimes(h, out c, out x, out k, out u);
            CloseHandle(h);
            return ok ? c : 0;
        }

        // Every process named like one of `roots` and installed under Folder,
        // and everything they started.
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
                    if (names.Contains(e.szExeFile) && PathOf(e.th32ProcessID).StartsWith(Folder, StringComparison.OrdinalIgnoreCase)) found.Add(e.th32ProcessID);
                } while (Process32Next(snap, ref e));
            }
            CloseHandle(snap);
            bool grew = found.Count > 0;
            while (grew)
            {
                grew = false;
                foreach (var kv in parents)
                {
                    if (found.Contains(kv.Key) || !found.Contains(kv.Value)) continue;
                    // A process older than its "parent" only shares a reused id.
                    long child = StartedAt(kv.Key), parent = StartedAt(kv.Value);
                    if (child != 0 && parent != 0 && child < parent) continue;
                    found.Add(kv.Key);
                    grew = true;
                }
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

        [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);

        const int GridW = 64, GridH = 40;

        // The app's largest visible window, as a 64x40 grid of brightness.
        // Null when it has no window yet.
        static byte[] Frame(string[] roots)
        {
            var pids = new HashSet<uint>();
            foreach (int p in Tree(roots)) pids.Add((uint)p);
            IntPtr best = IntPtr.Zero;
            int bw = 0, bh = 0;
            EnumWindows(delegate (IntPtr hwnd, IntPtr l)
            {
                if (!IsWindowVisible(hwnd)) return true;
                uint pid;
                GetWindowThreadProcessId(hwnd, out pid);
                if (!pids.Contains(pid)) return true;
                RECT r;
                if (!GetWindowRect(hwnd, out r)) return true;
                int w = r.Right - r.Left, h = r.Bottom - r.Top;
                if (w >= 300 && h >= 200 && (long)w * h > (long)bw * bh) { best = hwnd; bw = w; bh = h; }
                return true;
            }, IntPtr.Zero);
            if (best == IntPtr.Zero) return null;
            var grid = new byte[GridW * GridH];
            using (var full = new Bitmap(bw, bh, PixelFormat.Format32bppArgb))
            using (var small = new Bitmap(GridW, GridH, PixelFormat.Format32bppArgb))
            {
                using (var g = Graphics.FromImage(full))
                {
                    IntPtr dc = g.GetHdc();
                    PrintWindow(best, dc, 2);
                    g.ReleaseHdc(dc);
                }
                using (var g = Graphics.FromImage(small))
                {
                    g.InterpolationMode = InterpolationMode.HighQualityBilinear;
                    g.DrawImage(full, 0, 0, GridW, GridH);
                }
                var data = small.LockBits(new Rectangle(0, 0, GridW, GridH), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
                var raw = new byte[data.Stride * GridH];
                Marshal.Copy(data.Scan0, raw, 0, raw.Length);
                small.UnlockBits(data);
                for (int y = 0; y < GridH; y++)
                    for (int x = 0; x < GridW; x++)
                    {
                        int i = y * data.Stride + x * 4;
                        grid[y * GridW + x] = (byte)((raw[i] * 11 + raw[i + 1] * 59 + raw[i + 2] * 30) / 100);
                    }
            }
            return grid;
        }

        // More than 1% of the grid changed: a spinner or a caret is not a change.
        static bool Differs(byte[] a, byte[] b)
        {
            if (a == null || b == null) return a != b;
            int changed = 0;
            for (int i = 0; i < a.Length; i++) if (Math.Abs(a[i] - b[i]) > 10) changed++;
            return changed * 100 > a.Length;
        }

        // Something is drawn: at least 5% of the grid is not the background.
        static bool Drawn(byte[] a)
        {
            var counts = new int[256];
            foreach (byte v in a) counts[v]++;
            int mode = 0;
            for (int i = 1; i < 256; i++) if (counts[i] > counts[mode]) mode = i;
            int other = 0;
            foreach (byte v in a) if (Math.Abs(v - mode) > 10) other++;
            return other * 20 > a.Length;
        }

        // Starts the app and watches it. Returns, in ms from the launch (-1
        // where the timeout came first):
        //   [0] window: a window is on screen
        //   [1] drawn:  the window shows its content and then stays the same
        //               for three seconds (the time of the last change)
        //   [2] quiet:  start of the first second under 10% of one core
        //   [3] CPU seconds the app used until it was quiet
        public static double[] Launch(string exe, string args, string[] roots, int timeoutMs)
        {
            var psi = new ProcessStartInfo(exe, args);
            psi.UseShellExecute = true;
            var clock = Stopwatch.StartNew();
            Process.Start(psi);
            double window = -1, drawn = -1, quiet = -1, cpuAtQuiet = -1;
            while (clock.ElapsedMilliseconds < timeoutMs)
            {
                if (HasWindow(roots)) { window = clock.Elapsed.TotalMilliseconds; break; }
                Thread.Sleep(5);
            }
            if (window < 0) return new double[] { -1, -1, -1, -1 };

            byte[] last = null;
            double lastChange = window;
            int calm = 0;
            double streakStart = 0;
            double cpuWas = CpuSeconds(roots);
            double cpuAt = clock.Elapsed.TotalSeconds;
            while (clock.ElapsedMilliseconds < timeoutMs && (drawn < 0 || quiet < 0))
            {
                Thread.Sleep(50);
                double now = clock.Elapsed.TotalMilliseconds;
                if (drawn < 0)
                {
                    byte[] frame = Frame(roots);
                    if (Differs(frame, last)) lastChange = now;
                    last = frame;
                    if (frame != null && Drawn(frame) && now - lastChange >= 3000) drawn = lastChange;
                }
                if (quiet < 0 && now / 1000 - cpuAt >= 0.25)
                {
                    double cpu = CpuSeconds(roots);
                    double load = (cpu - cpuWas) / (now / 1000 - cpuAt);
                    if (load < 0.10)
                    {
                        if (calm == 0) streakStart = cpuAt * 1000;
                        calm++;
                        if (calm >= 4) { quiet = streakStart; cpuAtQuiet = cpu; }
                    }
                    else calm = 0;
                    cpuWas = cpu;
                    cpuAt = now / 1000;
                }
            }
            return new double[] { window, drawn, quiet, cpuAtQuiet };
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
        Roots = @('regards-desktop', 'regards'); Folder = "$regardsDir\"
        Install = { Get-FolderMB (Get-ChildItem $regardsDir -Filter *.exe).FullName }
        Data = { Get-FolderMB $regardsDir -Exclude '*.exe' }
        PreStop = { & (Join-Path $regardsDir 'regards.exe') daemon stop 2>$null | Out-Null }
    }
    Spark       = @{
        Exe = Join-Path $local 'Programs\SparkDesktop\Spark Desktop.exe'; Args = ''
        Roots = @('Spark Desktop'); Folder = (Join-Path $local 'ProgramsSparkDesktop') + ''
        Install = { Get-FolderMB (Join-Path $local 'Programs\SparkDesktop') }
        Data = { Get-FolderMB (Join-Path $roaming 'Spark Desktop'), (Join-Path $local 'Spark Desktop') }
    }
    Mailspring  = @{
        Exe = Join-Path $local 'Mailspring\mailspring.exe'; Args = ''
        Roots = @('mailspring'); Folder = (Join-Path $local 'Mailspring') + ''
        Install = { Get-FolderMB (Join-Path $local 'Mailspring') }
        Data = { Get-FolderMB (Join-Path $roaming 'Mailspring') }
    }
    Thunderbird = @{
        Exe = 'C:\Program Files\Mozilla Thunderbird\thunderbird.exe'; Args = ''
        Roots = @('thunderbird'); Folder = 'C:Program FilesMozilla Thunderbird'
        Install = { Get-FolderMB 'C:\Program Files\Mozilla Thunderbird' }
        Data = { Get-FolderMB (Join-Path $roaming 'Thunderbird'), (Join-Path $local 'Thunderbird') }
    }
    Outlook     = @{
        Exe = 'explorer.exe'; Args = 'shell:AppsFolder\Microsoft.OutlookForWindows_8wekyb3d8bbwe!Microsoft.OutlookforWindows'
        Roots = @('olk'); Folder = ''
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

    [Bench.Probe]::Folder = $app.Folder
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
        WindowMs = @(); DrawnMs = @(); QuietMs = @(); StartCpuS = @()
    }

    if ($Passive) {
        if (-not $wasRunning) { Write-Host "$name is not running, skipped (passive)"; continue }
    }
    else {
        for ($i = 0; $i -le $Runs; $i++) {
            Stop-App $app
            Start-Sleep -Seconds 3
            $t = [Bench.Probe]::Launch($app.Exe, $app.Args, $app.Roots, 60000)
            $label = if ($i -eq 0) { 'warm-up (not counted)' } else { "run $i" }
            Write-Host ("{0} {1}: window {2:n0} ms, drawn {3:n0} ms, quiet {4:n0} ms, cpu {5:n2} s" -f $name, $label, $t[0], $t[1], $t[2], $t[3])
            if ($i -eq 0) { continue }
            if ($t[0] -ge 0) { $row.WindowMs += [math]::Round($t[0]) }
            if ($t[1] -ge 0) { $row.DrawnMs += [math]::Round($t[1]) }
            if ($t[2] -ge 0) { $row.QuietMs += [math]::Round($t[2]); $row.StartCpuS += [math]::Round($t[3], 2) }
        }
        Start-Sleep -Seconds $SettleSeconds
    }

    $memory = Get-Memory $app
    $row.Processes = $memory.Processes
    $row.PrivateWorkingSetMB = $memory.PrivateWorkingSetMB
    $row.CommitMB = $memory.CommitMB
    $row.IdleCpuPercent = Get-IdleCpuPercent $app $IdleSeconds
    $row.WindowMsMedian = Get-Median $row.WindowMs
    $row.DrawnMsMedian = Get-Median $row.DrawnMs
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
    @{ n = 'Drawn ms'; e = { if ($null -ne $_.DrawnMsMedian) { [int]$_.DrawnMsMedian } } },
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
