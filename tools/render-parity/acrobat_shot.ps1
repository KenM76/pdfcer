<#
.SYNOPSIS
    Open a PDF in Adobe Acrobat and capture the Acrobat window as a PNG.

.DESCRIPTION
    WHAT IS ACTUALLY INSTALLED HERE: Adobe Acrobat **READER**, not Acrobat Pro.
    Adobe uses the same `\Acrobat DC\Acrobat\Acrobat.exe` path for both, and the
    paid features are licence-gated at runtime rather than absent from disk, so
    the path is NOT evidence of which product is present. Reader shares the
    rendering engine, which is the only thing this script uses and the only
    thing a render audit needs. It cannot verify any Acrobat *Pro* behaviour;
    a finding that depends on Pro capability must say it was not checkable
    here rather than let a Reader observation stand in for a Pro one.

    STRICTLY VIEW-ONLY. Do not drive Edit / Convert / Prepare / Redact /
    Preflight / Compare from this script or by hand while it runs: in Reader
    those raise a modal purchase prompt on the operator's real screen, which is
    useless to an audit and may not dismiss itself. This script only opens a
    document and asks its window to paint itself.

    A THIRD-OPINION tiebreaker for the render-parity audit. `render_parity.py`
    compares pdfcer against pdfium; when they disagree, nothing in that harness
    says WHICH is wrong. Acrobat is the project's actual parity target, so a
    visual look at Acrobat adjudicates the disagreement.

    This is a VISUAL ADJUDICATION, never a measurement. Acrobat renders at its
    own zoom, with its own chrome, on its own page background; no pixel metric
    computed against this capture would mean anything. The only questions it
    answers are categorical: is the rectangle filled or outlined, is the text
    spaced or stacked, is the colour obviously different.

    CAPTURE METHOD -- and why it is not a screen grab. A screen-region grab
    depends on `SetForegroundWindow`, which Windows refuses for a process that
    does not own the foreground (a documented anti-focus-stealing rule). The
    refusal is SILENT: the grab still succeeds and photographs whatever window
    actually had those screen coordinates, producing an image that looks like
    evidence and is not. `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` asks
    the window to draw ITSELF into an offscreen DC, so the pixels are that
    window's by construction, foreground or not. PW_RENDERFULLCONTENT (0x2) is
    required for DirectComposition/GPU-composited content, which Acrobat's
    document view uses; without it the document area comes back blank or black.

.PARAMETER Pdf
    Absolute path to the PDF to open.

.PARAMETER Out
    Absolute path of the PNG to write.

.PARAMETER WaitSeconds
    How long to wait for the document window to appear and settle. Acrobat's
    first launch is slow (splash + possible sign-in nag), later ones are fast.

.PARAMETER KeepOpen
    Leave Acrobat running. Default is to close the process this script started.

.EXAMPLE
    powershell -File acrobat_shot.ps1 -Pdf D:\a.pdf -Out D:\a.png
#>
param(
    [Parameter(Mandatory = $true)][string]$Pdf,
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$WaitSeconds = 25,
    [switch]$KeepOpen
)

$ErrorActionPreference = 'Stop'

$acro = 'C:\Program Files\Adobe\Acrobat DC\Acrobat\Acrobat.exe'
if (-not (Test-Path $acro)) { throw "Acrobat not found at $acro" }
if (-not (Test-Path $Pdf))  { throw "PDF not found: $Pdf" }

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win32Shot {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

# Launch. Acrobat single-instances itself: a second launch reuses the running
# process and opens a new tab, so the process we get back may not be the one
# that owns the window. We therefore locate the window by TITLE, not by the
# returned process handle.
#
# CAUTION, and why the fallback branch below prints the title it settled for:
# Acrobat titles the tab with the document's /Title (or XMP dc:title) when the
# file has one, NOT with the filename. `7.2-t31-pass-a.pdf` shows up as
# "Lang-Alt-pass". So a filename match failing does not mean the wrong document
# is open -- but it does mean the script can no longer PROVE the right one is.
# The fallback therefore reports the title it used, and the caller must confirm
# it (e.g. by grepping the file for /Title) before treating the capture as
# evidence. Closing every Acrobat process before a capture makes that
# confirmation sound, because then only one document can be open.
$leaf = [System.IO.Path]::GetFileNameWithoutExtension($Pdf)
$startedPid = $null
$before = @(Get-Process -Name Acrobat -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id)
$p = Start-Process -FilePath $acro -ArgumentList "`"$Pdf`"" -PassThru
$startedPid = $p.Id

$deadline = (Get-Date).AddSeconds($WaitSeconds)
$hwnd = [IntPtr]::Zero
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Milliseconds 700
    foreach ($proc in (Get-Process -Name Acrobat -ErrorAction SilentlyContinue)) {
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero -and
            $proc.MainWindowTitle -like "*$leaf*") {
            $hwnd = $proc.MainWindowHandle
            break
        }
    }
    if ($hwnd -ne [IntPtr]::Zero) { break }
}
if ($hwnd -eq [IntPtr]::Zero) {
    # Fall back to any visible Acrobat main window; report the title so the
    # caller can judge whether it is the right document (a first-run EULA or
    # updater dialog would show up here with a different title).
    foreach ($proc in (Get-Process -Name Acrobat -ErrorAction SilentlyContinue)) {
        if ($proc.MainWindowHandle -ne [IntPtr]::Zero) {
            $hwnd = $proc.MainWindowHandle
            Write-Host "WARN: title match failed; using window '$($proc.MainWindowTitle)'"
            break
        }
    }
}
if ($hwnd -eq [IntPtr]::Zero) { throw "no Acrobat window appeared within $WaitSeconds s" }

# Give the document view time to finish its first paint after the window
# exists; PrintWindow on a half-painted view yields a grey document area.
Start-Sleep -Seconds 4

$r = New-Object Win32Shot+RECT
[void][Win32Shot]::GetWindowRect($hwnd, [ref]$r)
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
if ($w -le 0 -or $h -le 0) { throw "window rect empty ($w x $h)" }

$bmp = New-Object System.Drawing.Bitmap($w, $h)
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $gfx.GetHdc()
$okShot = [Win32Shot]::PrintWindow($hwnd, $hdc, 2)   # 2 = PW_RENDERFULLCONTENT
$gfx.ReleaseHdc($hdc)
$gfx.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Host "saved $Out ($w x $h) printwindow=$okShot"

if (-not $KeepOpen) {
    foreach ($proc in (Get-Process -Name Acrobat -ErrorAction SilentlyContinue)) {
        if ($before -notcontains $proc.Id) {
            try { $proc.CloseMainWindow() | Out-Null; Start-Sleep -Milliseconds 800 } catch {}
            try { if (-not $proc.HasExited) { $proc.Kill() } } catch {}
        }
    }
}
