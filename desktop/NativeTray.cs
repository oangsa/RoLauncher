using System.Runtime.InteropServices;

namespace RoLauncher.Desktop;

// WinUI owns the message loop. A small subclass provides the system tray without another UI framework.
internal sealed class NativeTray : IDisposable
{
    private const uint Callback = 0x8004;
    private readonly nint _hwnd;
    private readonly SubclassProc _callback;
    private readonly Action _open, _exit;
    private NotifyIconData _icon;
    private readonly nint _appIcon;

    public NativeTray(nint hwnd, Action open, Action exit)
    {
        _hwnd = hwnd; _open = open; _exit = exit; _callback = Procedure;
        _appIcon = LoadImageW(0, Path.Combine(AppContext.BaseDirectory, "RoLauncher.ico"), 1, 32, 32, 0x10);
        _icon = new NotifyIconData
        {
            Size = (uint)Marshal.SizeOf<NotifyIconData>(), Window = hwnd, Id = 1,
            Flags = 1 | 2 | 4, CallbackMessage = Callback, Icon = _appIcon != 0 ? _appIcon : LoadIconW(0, (nint)32512),
            Tip = "RoLauncher — double-click to open", Info = "", InfoTitle = ""
        };
        SetWindowSubclass(hwnd, _callback, 1, 0);
        Shell_NotifyIconW(0, ref _icon);
    }
    private nint Procedure(nint hwnd, uint msg, nuint w, nint l, nuint id, nuint data)
    {
        if (msg == 0x0024) // WM_GETMINMAXINFO; preserve native DPI scaling.
        {
            var limits = Marshal.PtrToStructure<MinMaxInfo>(l);
            var scale = GetDpiForWindow(hwnd) / 96.0;
            limits.MinTrack = new Point { X = (int)(1020 * scale), Y = (int)(780 * scale) };
            Marshal.StructureToPtr(limits, l, false);
            return 0;
        }
        if (msg == Callback)
        {
            if ((uint)l == 0x0203) _open(); // Double click.
            if ((uint)l == 0x0205)
            {
                var menu = CreatePopupMenu();
                AppendMenuW(menu, 0, 1, "Open RoLauncher");
                AppendMenuW(menu, 0, 2, "Exit (leave clients running)");
                GetCursorPos(out var point); SetForegroundWindow(hwnd);
                var choice = TrackPopupMenu(menu, 0x0100 | 0x0080, point.X, point.Y, 0, hwnd, 0);
                DestroyMenu(menu);
                if (choice == 1) _open(); else if (choice == 2) _exit();
            }
            return 0;
        }
        return DefSubclassProc(hwnd, msg, w, l);
    }
    public void Dispose()
    {
        Shell_NotifyIconW(2, ref _icon);
        RemoveWindowSubclass(_hwnd, _callback, 1);
        if (_appIcon != 0) DestroyIcon(_appIcon);
        GC.KeepAlive(_callback);
    }
    public static void Message(string message) => MessageBoxW(0, message, "RoLauncher", 0x40);
    public static double Scale(nint hwnd) => GetDpiForWindow(hwnd) / 96.0;
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] private struct MinMaxInfo { public Point Reserved, MaxSize, MaxPosition, MinTrack, MaxTrack; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] private struct NotifyIconData
    {
        public uint Size; public nint Window; public uint Id, Flags, CallbackMessage; public nint Icon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string Tip;
        public uint State, StateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string Info;
        public uint Version;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string InfoTitle;
        public uint InfoFlags; public Guid Guid; public nint BalloonIcon;
    }
    [UnmanagedFunctionPointer(CallingConvention.Winapi)] private delegate nint SubclassProc(nint hwnd, uint msg, nuint w, nint l, nuint id, nuint data);
    [DllImport("comctl32.dll")] private static extern bool SetWindowSubclass(nint hwnd, SubclassProc callback, nuint id, nuint data);
    [DllImport("comctl32.dll")] private static extern bool RemoveWindowSubclass(nint hwnd, SubclassProc callback, nuint id);
    [DllImport("comctl32.dll")] private static extern nint DefSubclassProc(nint hwnd, uint msg, nuint w, nint l);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] private static extern bool Shell_NotifyIconW(uint command, ref NotifyIconData data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern nint LoadIconW(nint module, nint name);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern nint LoadImageW(nint module, string name, uint type, int width, int height, uint flags);
    [DllImport("user32.dll")] private static extern bool DestroyIcon(nint icon);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(nint hwnd);
    [DllImport("user32.dll")] private static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(nint hwnd);
    [DllImport("user32.dll")] private static extern nint CreatePopupMenu();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern bool AppendMenuW(nint menu, uint flags, nuint id, string text);
    [DllImport("user32.dll")] private static extern uint TrackPopupMenu(nint menu, uint flags, int x, int y, int reserved, nint hwnd, nint rect);
    [DllImport("user32.dll")] private static extern bool DestroyMenu(nint menu);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern int MessageBoxW(nint hwnd, string text, string caption, uint flags);
}
