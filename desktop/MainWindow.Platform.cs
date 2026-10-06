using Microsoft.UI;
using Microsoft.UI.Windowing;
using Windows.System;
using WinRT.Interop;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private IDisposable ConfigureDesktop()
    {
        var hwnd = WindowNative.GetWindowHandle(this);
        var appWindow = AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd));
        appWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "RoLauncher.ico"));
        var scale = NativeTray.Scale(hwnd);
        var work = DisplayArea.GetFromWindowId(appWindow.Id, DisplayAreaFallback.Primary).WorkArea;
        appWindow.Resize(new Windows.Graphics.SizeInt32(Math.Min((int)(1180 * scale), work.Width - 48),
            Math.Min((int)(940 * scale), work.Height - 48)));
        appWindow.Closing += (sender, e) =>
        {
            if (_exiting) return;
            e.Cancel = true;
            appWindow.Hide();
        };
        return new NativeTray(hwnd, () => { appWindow.Show(); Activate(); }, async () => await ExitAsync());
    }

    private void InitializePicker(object picker) => InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
    private void SelectAllRows() => AccountList.SelectAll();
    private async Task ShowAccountDialogAsync() => await AccountDialog.ShowAsync();
    private static bool IsControlDown() => Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control)
        .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
}
