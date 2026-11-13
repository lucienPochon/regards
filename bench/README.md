# The mail app benchmark

How Regards is compared with other mail apps, and the rules a number must
follow before it is published. Everything is measured from the outside, the
same way for every app, on the same mail. Nothing here reads an app's mail or
its files.

- **`seed-mailbox.mjs`** fills an empty mailbox with the benchmark's fixed set
  of messages.
- **`compare-apps.ps1`** launches each installed app and measures it (Windows).

## What is measured

| Number | What it is | How |
| --- | --- | --- |
| Window | Launch until a window is on screen | Polled every 5 ms |
| Drawn | Launch until the window shows its content and stops changing | The window is captured about 15 times a second as a 64×40 grid. The time is that of the last change (over 1% of the grid) before three still seconds, with something on screen (over 5% of the grid is not background) |
| Quiet | Launch until the app stops working | Start of the first second under 10% of one core |
| CPU to start | Processor time used until quiet | Sum over the app's processes |
| Memory | What the app holds 60 s after launch | Private working set, summed over the app's processes |
| Idle CPU | What it uses doing nothing | Processor time over the next 60 s, as a share of one core |
| Processes | How many it runs | Counted with the memory |
| Install size | Size of the installed program | Files in its install folder |

An app is its own programs, found by their install folder, and every process
they start. Regards draws with the WebView2 that comes with Windows: those
processes are counted in its memory and CPU, and WebView2 is not in its install
size, because it is already on the computer.

Window and Drawn are the median of 10 launches. One more launch is made first
and not counted. Quiet and CPU to start are the median of 5 further launches
during which the window is not captured: capturing a window makes the app draw,
which is work it would not do alone. The lowest and highest are published with the median. A launch
here is a warm one: the app has run before since the computer started. A first
launch after a restart is slower for every app and is not measured.

## The rules for published numbers

1. **The same mail.** Every app has one account and nothing else: a mailbox
   filled by `seed-mailbox.mjs` (set `regards-bench-1`: 20,000 messages,
   333 MB, in Inbox, Archive and Sent; three in ten are replies, 5% are long
   HTML newsletters, 3% carry a file). The script prints the set's
   fingerprint, which is published with the results:
   `sha256:0ade4a505daab00d62a3c5d5b7613fd831e4378233e76d87490938548ec61a80`.
2. **A clean user.** A Windows user made for the benchmark, with each app
   freshly installed in its latest released version and left on its default
   settings. Add the account, then leave each app open until it has finished
   fetching the mailbox.
3. **A quiet computer.** Plugged in, nothing else open, no build or update
   running, one app at a time.
4. **Everything is stated.** The page gives the date, the computer, the Windows
   version, each app's version, and the version of this folder (the commit).
   The raw result file is kept in `bench/results/`.
5. **Every app, every number.** No app and no measurement is left out because
   Regards loses it.
6. **Run again at each release** of Regards, and when another app has a major
   release.

## Running it

```powershell
# 1. Once: fill the mailbox (any IMAP account that is empty).
$env:BENCH_IMAP_HOST = 'mail.example.org'
$env:BENCH_IMAP_USER = 'bench@example.org'
$env:BENCH_IMAP_PASSWORD = '...'
node bench\seed-mailbox.mjs

# 2. As the benchmark's Windows user, with the account added in each app:
powershell -ExecutionPolicy Bypass -File bench\compare-apps.ps1 -Out bench\results\2026-11-13.json
```

`compare-apps.ps1` closes each app before every launch (it asks the window to
close, then ends the process), so do not run it while a message is being sent.
Apps that were open before are opened again at the end. `-Passive` measures
only what is already running and starts or stops nothing. `-Apps` chooses the
apps; `Outlook` (the new Outlook for Windows) is known but not in the default
list.

## What it does not tell

- How fast an app fetches a mailbox for the first time, opens a message or
  searches. Those need each app driven by hand or by its own interface, and
  are not measured yet.
- Anything about macOS or Linux.
- Regards' own speed on a large mailbox is tested separately, without the
  app's window: `cargo test -p regards-store --release --test scale -- --ignored --nocapture`.
