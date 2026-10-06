using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;

namespace RoLauncher.Desktop;

/// Ayatana AppIndicator exports the same menu on KDE and GNOME's indicator host.
/// Only hide the window after the indicator has connected to a tray host.
internal sealed class LinuxTray : IDisposable
{
    private readonly DispatcherTimer _events = new() { Interval = TimeSpan.FromMilliseconds(100) };
    private readonly ActivateCallback _open, _exit;
    private nint _library, _indicator, _menu;
    private SetStatus? _setStatus;
    private bool _disposed, _connected;

    public bool IsConnected
    {
        get
        {
            if (_indicator == 0 || _disposed) return false;
            g_object_get(_indicator, "connected", out var connected, 0);
            return connected != 0;
        }
    }

    public LinuxTray(Action open, Action exit)
    {
        _open = (_, _) => open(); _exit = (_, _) => exit();
        try
        {
            if (!NativeLibrary.TryLoad("libayatana-appindicator3.so.1", out _library)
                && !NativeLibrary.TryLoad("libappindicator3.so.1", out _library)) return;
            if (gtk_init_check(0, 0) == 0) return;
            var create = Load<NewIndicator>("app_indicator_new");
            _setStatus = Load<SetStatus>("app_indicator_set_status");
            _indicator = create("rolauncher", Path.Combine(AppContext.BaseDirectory, "RoLauncher.png"), 0);
            if (_indicator == 0) return;
            _menu = gtk_menu_new();
            // Keep our own reference; the indicator sinks/owns a separate reference.
            g_object_ref_sink(_menu);
            Add("Open RoLauncher", _open);
            Add("Exit (leave clients running)", _exit);
            gtk_widget_show_all(_menu);
            Load<SetMenu>("app_indicator_set_menu")(_indicator, _menu);
            _setStatus(_indicator, 1);
            _events.Tick += (_, _) =>
            {
                for (var i = 0; i < 20 && g_main_context_iteration(0, 0) != 0; i++) { }
                // A disappearing tray host must not leave an inaccessible hidden window.
                var connected = IsConnected;
                if (_connected && !connected) open();
                _connected = connected;
            };
            _events.Start();
        }
        catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException or BadImageFormatException)
        { Dispose(); }
    }

    private T Load<T>(string name) where T : Delegate => Marshal.GetDelegateForFunctionPointer<T>(NativeLibrary.GetExport(_library, name));
    private void Add(string title, ActivateCallback callback)
    {
        var item = gtk_menu_item_new_with_label(title);
        gtk_menu_shell_append(_menu, item);
        g_signal_connect_data(item, "activate", callback, 0, 0, 0);
    }
    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true; _events.Stop();
        if (_indicator != 0) { _setStatus?.Invoke(_indicator, 0); g_object_unref(_indicator); _indicator = 0; }
        if (_menu != 0) { gtk_widget_destroy(_menu); g_object_unref(_menu); _menu = 0; }
        // Keep the native module loaded while GObject may still dispatch queued callbacks.
        GC.KeepAlive(_open); GC.KeepAlive(_exit);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ActivateCallback(nint widget, nint data);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate nint NewIndicator([MarshalAs(UnmanagedType.LPUTF8Str)] string id, [MarshalAs(UnmanagedType.LPUTF8Str)] string icon, int category);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void SetStatus(nint indicator, int status);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void SetMenu(nint indicator, nint menu);
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern int gtk_init_check(nint argc, nint argv);
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern nint gtk_menu_new();
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern nint gtk_menu_item_new_with_label([MarshalAs(UnmanagedType.LPUTF8Str)] string label);
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern void gtk_menu_shell_append(nint menu, nint item);
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern void gtk_widget_show_all(nint widget);
    [DllImport("libgtk-3.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern void gtk_widget_destroy(nint widget);
    [DllImport("libgobject-2.0.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern ulong g_signal_connect_data(nint obj, [MarshalAs(UnmanagedType.LPUTF8Str)] string signal, ActivateCallback callback, nint data, nint notify, int flags);
    [DllImport("libgobject-2.0.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern void g_object_get(nint obj, [MarshalAs(UnmanagedType.LPUTF8Str)] string name, out int value, nint end);
    [DllImport("libgobject-2.0.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern void g_object_unref(nint obj);
    [DllImport("libgobject-2.0.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern nint g_object_ref_sink(nint obj);
    [DllImport("libglib-2.0.so.0", CallingConvention = CallingConvention.Cdecl)] private static extern int g_main_context_iteration(nint context, int block);
}
