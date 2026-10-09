using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Windows.System;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private IDisposable ConfigureDesktop()
    {
        // Uno has no Windows Mica backdrop. Use its neutral solid fallback base.
        Root.ActualThemeChanged += (_, _) => ApplyLinuxWorkspaceSurface();
        ApplyLinuxWorkspaceSurface();
        // Uno initially lays an inline ContentDialog out as page content. Remove it
        // from the visual tree; ShowAsync attaches it to its own popup when needed.
        Root.Children.Remove(_accountDialog);
        Root.Children.Remove(_presetLookupDialog);
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
            if (_saving) { e.Cancel = true; return; }
            if (!_closeToTray) { _exiting = true; return; }
            e.Cancel = true;
            if (tray.IsConnected) window.Hide();
            else (AppWindow.Presenter as OverlappedPresenter)?.Minimize();
        };
        return new LinuxLifetime(tray, window);
    }

    // Uno's Linux pickers use the session's XDG desktop portal (GNOME or KDE).
    private static void InitializePicker(object picker) { }
    private static bool NavigationAnimationsEnabled() => new Windows.UI.ViewManagement.UISettings().AnimationsEnabled;
    private void ApplyLinuxWorkspaceSurface() => Root.Background = new SolidColorBrush(Root.ActualTheme == ElementTheme.Dark
        ? Windows.UI.Color.FromArgb(255, 32, 32, 32) : Windows.UI.Color.FromArgb(255, 243, 243, 243));
    private async Task ShowAccountDialogAsync() => await _accountDialog.ShowAsync();
    private void SelectAllRows()
    {
        foreach (var row in _visibleRows)
            row.IsSelected = true;
        UpdateSelectionSettings();
    }
    private static bool IsControlDown() => Microsoft.UI.Xaml.Window.Current?.CoreWindow?.GetKeyState(VirtualKey.Control)
        .HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down) == true;
    private sealed class LinuxLifetime(LinuxTray tray, X11Visibility window) : IDisposable
    { public void Dispose() { tray.Dispose(); window.Dispose(); } }
}
