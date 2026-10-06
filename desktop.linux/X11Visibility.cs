using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Uno.UI.NativeElementHosting;
using Uno.UI.Xaml;

namespace RoLauncher.Desktop;

/// Use Uno's public native handle because AppWindow.Hide is unimplemented on X11.
internal sealed class X11Visibility(Window window) : IDisposable
{
    private readonly nint _display = XOpenDisplay(0);
    public void Hide() => SetVisible(false);
    public void Show() => SetVisible(true);
    private void SetVisible(bool visible)
    {
        if (_display == 0 || window.GetNativeWindow() is not X11NativeWindow native) return;
        if (visible) XMapWindow(_display, native.WindowId);
        else XUnmapWindow(_display, native.WindowId);
        XFlush(_display);
    }
    public void Dispose() { if (_display != 0) XCloseDisplay(_display); }
    [DllImport("libX11.so.6")] private static extern nint XOpenDisplay(nint display);
    [DllImport("libX11.so.6")] private static extern int XMapWindow(nint display, nint window);
    [DllImport("libX11.so.6")] private static extern int XUnmapWindow(nint display, nint window);
    [DllImport("libX11.so.6")] private static extern int XFlush(nint display);
    [DllImport("libX11.so.6")] private static extern int XCloseDisplay(nint display);
}
