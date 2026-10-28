# Windows testi yardimcilari: ekran goruntusu, gercek klavye/fare girdisi, ondeki pencere.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Girdi {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr hwnd, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateCompatibleDC(IntPtr dc);
    [DllImport("gdi32.dll")] public static extern IntPtr CreateCompatibleBitmap(IntPtr dc, int w, int h);
    [DllImport("gdi32.dll")] public static extern IntPtr SelectObject(IntPtr dc, IntPtr obj);
    [DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
    [DllImport("gdi32.dll")] public static extern bool DeleteObject(IntPtr obj);
    [DllImport("gdi32.dll")] public static extern bool DeleteDC(IntPtr dc);
    // SRCCOPY | CAPTUREBLT: katmanli (seffaf) pencereler de goruntuye girer.
    // Cagiran bitmap'i kaydedip DeleteObject ile birakir.
    public static IntPtr CaptureScreen(int w, int h) {
        IntPtr screen = GetDC(IntPtr.Zero);
        IntPtr memory = CreateCompatibleDC(screen);
        IntPtr bitmap = CreateCompatibleBitmap(screen, w, h);
        IntPtr old = SelectObject(memory, bitmap);
        BitBlt(memory, 0, 0, w, h, screen, 0, 0, 0x00CC0020 | 0x40000000);
        SelectObject(memory, old);
        DeleteDC(memory);
        ReleaseDC(IntPtr.Zero, screen);
        return bitmap;
    }
    public delegate bool EnumProc(IntPtr hwnd, IntPtr param);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc proc, IntPtr param);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr hwnd, int index);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool GetLayeredWindowAttributes(IntPtr hwnd, out uint key, out byte alpha, out uint flags);
    [DllImport("user32.dll")] public static extern bool SetLayeredWindowAttributes(IntPtr hwnd, uint key, byte alpha, uint flags);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public static string Title(IntPtr hwnd) {
        var text = new StringBuilder(512);
        GetWindowText(hwnd, text, text.Capacity);
        return text.ToString();
    }
    // Surecin ust duzey pencereleri: hwnd, baslik, konum, gorunurluk, genisletilmis stil.
    public static System.Collections.Generic.List<IntPtr> Windows(int pid) {
        var list = new System.Collections.Generic.List<IntPtr>();
        EnumWindows((hwnd, p) => { uint owner; GetWindowThreadProcessId(hwnd, out owner); if (owner == pid) list.Add(hwnd); return true; }, IntPtr.Zero);
        return list;
    }
    public static string Describe(IntPtr hwnd) {
        RECT r; GetWindowRect(hwnd, out r);
        uint key; byte alpha; uint flags;
        bool layered = GetLayeredWindowAttributes(hwnd, out key, out alpha, out flags);
        return string.Format("{0} '{1}' ({2},{3})-({4},{5}) gorunur={6} exstyle=0x{7:X8} katman={8}/{9}/{10}",
            hwnd, Title(hwnd), r.Left, r.Top, r.Right, r.Bottom, IsWindowVisible(hwnd), GetWindowLong(hwnd, -20), layered, alpha, flags);
    }
    public static string ForegroundTitle() {
        var text = new StringBuilder(512);
        GetWindowText(GetForegroundWindow(), text, text.Capacity);
        return text.ToString();
    }
}
"@

function Save-Screen([string]$path) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $hbitmap = [Girdi]::CaptureScreen($bounds.Width, $bounds.Height)
    $image = [System.Drawing.Image]::FromHbitmap($hbitmap)
    $image.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $image.Dispose()
    [Girdi]::DeleteObject($hbitmap) | Out-Null
    Write-Host "Ekran goruntusu: $path"
}

function Send-Keys([string]$keys) {
    [System.Windows.Forms.SendKeys]::SendWait($keys)
}

function Click-At([int]$x, [int]$y) {
    [Girdi]::SetCursorPos($x, $y) | Out-Null
    Start-Sleep -Milliseconds 100
    [Girdi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [Girdi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}

function Drag-Mouse([int]$x1, [int]$y1, [int]$x2, [int]$y2) {
    [Girdi]::SetCursorPos($x1, $y1) | Out-Null
    Start-Sleep -Milliseconds 200
    [Girdi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    for ($i = 1; $i -le 20; $i++) {
        $x = [int]($x1 + ($x2 - $x1) * $i / 20)
        $y = [int]($y1 + ($y2 - $y1) * $i / 20)
        [Girdi]::SetCursorPos($x, $y) | Out-Null
        Start-Sleep -Milliseconds 30
    }
    [Girdi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}

function Get-ForegroundTitle {
    [Girdi]::ForegroundTitle()
}

function Minimize-Window([IntPtr]$hwnd) {
    [Girdi]::ShowWindow($hwnd, 6) | Out-Null
}

function Get-AppWindows([int]$processId) {
    [Girdi]::Windows($processId) | Where-Object { [Girdi]::IsWindowVisible($_) }
}

function Show-AppWindows([int]$processId) {
    foreach ($hwnd in [Girdi]::Windows($processId)) { Write-Host ("  " + [Girdi]::Describe($hwnd)) }
}
