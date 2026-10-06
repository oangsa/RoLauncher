using Microsoft.UI.Windowing;
using Windows.System;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private IDisposable ConfigureDesktop()
    {
        // Uno initially lays an inline ContentDialog out as page content. Remove it
        // from the visual tree; ShowAsync attaches it to its own popup when needed.
        Root.Children.Remove(AccountDialog);
        BackupDescription.Text = BackupDescription.Text.Replace("Windows user", "Linux user", StringComparison.Ordinal);
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = 1180, Height = 940 });
        var window = new X11Visibility(this);
        var tray = new LinuxTray(() => DispatcherQueue.TryEnqueue(() => { window.Show(); AppWindow.Show(); Activate(); }),
            () => DispatcherQueue.TryEnqueue(async () => await ExitAsync()));
        // Until a tray host has acknowledged registration, closing must never strand
        // the supervisor in an invisible window (stock GNOME has no tray host).
        AppWindow.Closing += (_, e) =>
        {
            if (_exiting) return;
            e.Cancel = true;
            if (tray.IsConnected) window.Hide();
            else (AppWindow.Presenter as OverlappedPresenter)?.Minimize();
        };
        return new LinuxLifetime(tray, window);
    }

    // Uno's Linux pickers use the session's XDG desktop portal (GNOME or KDE).
    private static void InitializePicker(object picker) { }
    private async Task ShowAccountDialogAsync() => await AccountDialog.ShowAsync();
    private void SelectAllRows()
    {
        foreach (var row in _visibleRows)
            if (!AccountList.SelectedItems.Contains(row)) AccountList.SelectedItems.Add(row);
    }
    private static bool IsControlDown() => Microsoft.UI.Xaml.Window.Current?.CoreWindow?.GetKeyState(VirtualKey.Control)
        .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down) == true;
    private sealed class LinuxLifetime(LinuxTray tray, X11Visibility window) : IDisposable
    { public void Dispose() { tray.Dispose(); window.Dispose(); } }
}
