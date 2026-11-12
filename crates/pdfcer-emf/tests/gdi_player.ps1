# GDI's PlayEnhMetaFile onto a white 32 bpp DIB; raw BGRA to -Out. Used by the
# pdfcer-emf tests and tools/print-parity-report.py.
param([string]$Path, [int]$W, [int]$H, [string]$Out)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Gdi {
  [DllImport("gdi32.dll")] public static extern IntPtr CreateCompatibleDC(IntPtr hdc);
  [DllImport("gdi32.dll")] public static extern IntPtr CreateDIBSection(IntPtr hdc, ref BITMAPINFO bmi, uint usage, out IntPtr bits, IntPtr section, uint offset);
  [DllImport("gdi32.dll")] public static extern IntPtr SelectObject(IntPtr hdc, IntPtr h);
  [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr h);
  [DllImport("gdi32.dll")] public static extern bool DeleteDC(IntPtr hdc);
  [DllImport("gdi32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr GetEnhMetaFileW(string path);
  [DllImport("gdi32.dll")] public static extern bool PlayEnhMetaFile(IntPtr hdc, IntPtr hemf, ref RECT rect);
  [DllImport("gdi32.dll")] public static extern bool DeleteEnhMetaFile(IntPtr hemf);
  [DllImport("gdi32.dll")] public static extern IntPtr CreateSolidBrush(uint color);
  [DllImport("user32.dll")] public static extern int FillRect(IntPtr hdc, ref RECT rect, IntPtr brush);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct BITMAPINFO { public uint size; public int w, h; public ushort planes, bpp; public uint comp, sizeImage; public int xppm, yppm; public uint used, imp; public uint r0, g0, b0, a0; }
  public static byte[] Play(string path, int w, int h) {
    IntPtr hdc = CreateCompatibleDC(IntPtr.Zero);
    BITMAPINFO bmi = new BITMAPINFO(); bmi.size = 40; bmi.w = w; bmi.h = -h; bmi.planes = 1; bmi.bpp = 32; bmi.comp = 0;
    IntPtr bits; IntPtr hbm = CreateDIBSection(hdc, ref bmi, 0, out bits, IntPtr.Zero, 0);
    IntPtr old = SelectObject(hdc, hbm);
    RECT rc = new RECT(); rc.L = 0; rc.T = 0; rc.R = w; rc.B = h;
    IntPtr white = CreateSolidBrush(0x00FFFFFF); FillRect(hdc, ref rc, white); DeleteObject(white);
    IntPtr hemf = GetEnhMetaFileW(path);
    if (hemf == IntPtr.Zero) throw new Exception("GetEnhMetaFile failed");
    if (!PlayEnhMetaFile(hdc, hemf, ref rc)) throw new Exception("PlayEnhMetaFile failed");
    byte[] px = new byte[w * h * 4]; Marshal.Copy(bits, px, 0, px.Length);
    DeleteEnhMetaFile(hemf); SelectObject(hdc, old); DeleteObject(hbm); DeleteDC(hdc);
    return px;
  }
}
"@
[System.IO.File]::WriteAllBytes($Out, [Gdi]::Play($Path, $W, $H))
