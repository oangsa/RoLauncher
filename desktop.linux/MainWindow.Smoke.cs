#if LINUX_UI_SMOKE
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using SkiaSharp;
using Windows.Storage.Streams;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Uno.UI.NativeElementHosting;
using Uno.UI.Xaml;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private async Task LinuxSmokeAsync()
    {
        var directory = Environment.GetEnvironmentVariable("ROLAUNCHER_UI_SMOKE_DIR")!;
        Directory.CreateDirectory(directory);
        try
        {
            for (var i = 0; i < 100 && (_rows.Count == 0 || _refreshing); i++) await Task.Delay(100);
            _timer.Stop();
            Check(_rows.Count == 3 && !_refreshing, "Simulated accounts load through the authenticated API.");
            Check(_config.Version == GetType().Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false).Cast<System.Reflection.AssemblyInformationalVersionAttribute>().Single().InformationalVersion.Split('+')[0], "Version matches the supervisor.");
            Root.UpdateLayout();
            Check(Math.Abs(GroupFilter.ActualHeight - BulkEditButton.ActualHeight) < 0.5 && GroupFilter.ActualHeight > 0, "The group dropdown matches the adjacent button height.");
            await CaptureLinuxAsync(directory, "accounts.png");
            Tab_Click(PresetsTab, new RoutedEventArgs());
            var lookup = ShowPresetLookupAsync();
            await WaitLinuxAsync(() => _presetLookupDialog.ActualWidth > 0, "Linux lookup modal opens.");
            PresetSearch.Text = "alpha"; await Task.Delay(100);
            PresetSelectVisible_Click(PresetAccounts, new RoutedEventArgs());
            Check(_presetVisibleRows.Count == 1 && PresetLookupIds.Count == 1 && PresetSelected().Length == 0, "Preset lookup selects the searched account.");
            PresetSearch.Text = "beta"; await Task.Delay(100);
            Check(PresetLookupIds.Count == 1 && PresetLookupSummary.Text.Contains("hidden"), "Lookup choices survive Linux filtering.");
            PresetSearch.Text = ""; await Task.Delay(100);
            Root.UpdateLayout();
            Check(PresetTableHeader.ColumnDefinitions.Count == 6 && PresetTable.ActualWidth >= 640, "Linux renders the six-column account lookup table.");
            await CaptureLinuxAsync(directory, "preset-account-lookup.png", _presetLookupDialog);
            _presetLookupDialog.Hide(); await lookup;
            Check(PresetSelected().Length == 0 && !_modalOpen, "Cancel discards Linux lookup choices.");
            lookup = ShowPresetLookupAsync();
            await WaitLinuxAsync(() => _presetLookupDialog.ActualWidth > 0, "Linux lookup reopens.");
            PresetSearch.Text = "alpha"; await Task.Delay(100);
            PresetSelectVisible_Click(PresetAccounts, new RoutedEventArgs());
            Root.UpdateLayout();
            var useAccounts = FindLinux<Button>(_presetLookupDialog, "PrimaryButton") ?? throw new InvalidOperationException("Lookup confirmation button missing.");
            SendPointerClick(useAccounts); await lookup.WaitAsync(TimeSpan.FromSeconds(5));
            Check(PresetSelected().Length == 1 && !_modalOpen, "Native Linux lookup confirmation commits the selected accounts.");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            NameSearch.Text = "alpha";
            ApplyFilters();
            Check(_visibleRows.Count == 1, "The shared name filter works.");
            SelectAllRows();
            Check(Selected().Length == 1 && StartButton.IsEnabled, "Selection controls share the Windows behavior.");
            ClearFilters_Click(SettingsTab, new RoutedEventArgs());
            Check(_visibleRows.Count == 3, "Clearing filters restores saved accounts.");
            AccountList.SelectedItems.Clear(); LoginButton.Focus(FocusState.Programmatic);
            SendSelectAllKeys(); await Task.Delay(200);
            Check(Selected().Length == 3, "Native Linux Ctrl+A selects visible accounts.");
            var trace = new List<string>();
            Root.AddHandler(UIElement.PointerPressedEvent, new Microsoft.UI.Xaml.Input.PointerEventHandler((_, args) => trace.Add($"Pressed: {args.OriginalSource?.GetType().Name} {(args.OriginalSource as FrameworkElement)?.Name}")), true);
            SendPointerClick(StartButton); await Task.Delay(300);
            var rowButton = FindLinux<Button>(AccountList, "RowDetailsButton")!;
            Root.UpdateLayout();
            SendPointerClick(rowButton);
            await Task.Delay(200);
            File.WriteAllText(Path.Combine(directory, "pointer-trace.txt"), string.Join("\n", trace) + $"\nrow {rowButton.ActualWidth}x{rowButton.ActualHeight}; tag {rowButton.Tag}; modal {_modalOpen}; feedback {Feedback.Message}");
            Check(_modalOpen && _editorRow == ResolveAccount(rowButton), "Actual row edit button resolves the clicked account.");
            _accountDialog.Hide();
            await WaitLinuxAsync(() => !_modalOpen, "The row editor finishes closing before another dialog opens.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            PageScroll.ChangeView(null, 10000, null, true); await Task.Delay(150);
            Tab_Click(AccountsTab, new RoutedEventArgs()); await Task.Delay(150);
            Check(PageScroll.VerticalOffset == 0, "Switching pages resets Linux scrolling.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            await CaptureLinuxAsync(directory, "settings.png");
            Tab_Click(RecoveryTab, new RoutedEventArgs());
            await Task.Delay(150);
            await CaptureLinuxAsync(directory, "recovery.png");
            var changelog = ShowChangelogAsync();
            await WaitLinuxAsync(() => _changelogDialog is { ActualWidth: > 0, ActualHeight: > 0 }, "The changelog dialog opens.");
            var changelogDialog = _changelogDialog!;
            await CaptureLinuxAsync(directory, "changelog.png", changelogDialog);
            changelogDialog.Hide(); await changelog;
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var originalAlias = _rows[0].Alias;
            var showing = ShowAccountAsync(_rows[0]);
            await Task.Delay(250);
            Check(_accountDialog.ActualWidth > 0 && _accountDialog.ActualHeight > 0, "The account dialog opens in a popup.");
            Check(DialogFields.DataContext == _rows[0] && AliasInput.Text == originalAlias, "The shared editor binds the selected account.");
            AliasInput.Text = "Cancelled fixture edit";
            await CaptureLinuxAsync(directory, "account-dialog.png", _accountDialog);
            _accountDialog.Hide();
            await showing;
            Check(!_modalOpen && _rows[0].Alias == originalAlias, "Cancel closes the editor without saving edits.");
            showing = ShowAccountAsync(_rows[0]);
            await Task.Delay(150);
            PlaceInput.Text = "invalid place";
            Check(!await SaveEditorAsync() && EditorError.IsOpen && _modalOpen, "Invalid editor changes keep the dialog open.");
            PlaceInput.Text = "1";
            AliasInput.Text = "Saved Linux fixture";
            Check(await SaveEditorAsync(), "Valid editor changes reach the authenticated API.");
            _accountDialog.Hide();
            await showing;
            Check(_rows[0].Alias == "Saved Linux fixture", "Saved account edits are refreshed from the API.");
            showing = ShowBulkAsync(_rows.ToArray());
            await Task.Delay(200);
            Check(_bulkDialog is { ActualWidth: > 0 } && _bulkForm is not null && _modalOpen, "The shared bulk editor opens.");
            _bulkForm!.GroupApply.IsChecked = true;
            _bulkForm.Group.Text = "Cancelled group";
            await CaptureLinuxAsync(directory, "bulk-settings.png", _bulkDialog!);
            _bulkDialog!.Hide();
            await showing;
            Check(!_modalOpen && _rows.All(row => row.Account.Group == "Fixture"), "Cancelling bulk edits preserves saved settings.");
            FeedbackMessage("Account changes saved.", Microsoft.UI.Xaml.Controls.InfoBarSeverity.Success);
            await Task.Delay(100);
            await CaptureLinuxAsync(directory, "feedback.png");
            Check(Feedback.IsOpen, "Feedback uses the shared overlay.");
            File.WriteAllText(Path.Combine(directory, "result.txt"), "Linux shell smoke passed: native Ctrl+A, actual account row and Start buttons, page scroll reset, account Save/Cancel, bulk Cancel, and rendering. Native desktop parity and live Sober remain pending.");
        }
        catch (Exception e)
        {
            // Never include API tokens, cookies, request bodies or raw process arguments.
            File.WriteAllText(Path.Combine(directory, "result.txt"), "FAILED: " + e.GetType().Name + ": " + e.Message + "\n" + e.StackTrace);
        }
        finally { _exiting = true; Close(); }
    }

    private static T? FindLinux<T>(DependencyObject element, string name) where T : FrameworkElement
    {
        if (element is T item && item.Name == name) return item;
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(element); i++)
            if (FindLinux<T>(VisualTreeHelper.GetChild(element, i), name) is { } found) return found;
        return null;
    }
    // Native X11 events exercise Uno's key/modifier path rather than directly
    // calling the selection handler. No real accounts or game clients are used.
    private void SendSelectAllKeys()
    {
        var display = XOpenDisplay(0);
        if (display == 0 || this.GetNativeWindow() is not X11NativeWindow window) throw new InvalidOperationException("X11 test window missing.");
        var buffer = Marshal.AllocHGlobal(192);
        try {
            foreach (var (symbol, type, state) in new[] { (0xffe3u, 2, 0u), (0x61u, 2, 4u), (0x61u, 3, 4u), (0xffe3u, 3, 4u) }) {
                var key = new XKeyEvent { Type = type, Display = display, Window = window.WindowId, Keycode = XKeysymToKeycode(display, symbol), State = state, SameScreen = 1 };
                Marshal.StructureToPtr(key, buffer, false);
                XSendEvent(display, window.WindowId, 0, type == 2 ? 1 : 2, buffer);
            }
            XFlush(display);
        }
        finally { Marshal.FreeHGlobal(buffer); XCloseDisplay(display); }
    }
    private void SendPointerClick(FrameworkElement element)
    {
        var display = XOpenDisplay(0);
        if (display == 0 || this.GetNativeWindow() is not X11NativeWindow window) throw new InvalidOperationException("X11 test window missing.");
        var point = element.TransformToVisual(Root).TransformPoint(new Windows.Foundation.Point(element.ActualWidth / 2, element.ActualHeight / 2));
        try {
            XTranslateCoordinates(display, window.WindowId, XDefaultRootWindow(display), 0, 0, out var x, out var y, out _);
            var scale = Root.XamlRoot?.RasterizationScale ?? 1;
            XTestFakeMotionEvent(display, -1, x + (int)(point.X * scale), y + (int)(point.Y * scale), 0);
            XTestFakeButtonEvent(display, 1, 1, 0); XTestFakeButtonEvent(display, 1, 0, 0);
            XFlush(display);
        }
        finally { XCloseDisplay(display); }
    }
    [DllImport("libX11.so.6")] private static extern nint XDefaultRootWindow(nint display);
    [DllImport("libX11.so.6")] private static extern int XTranslateCoordinates(nint display, nint source, nint destination, int x, int y, out int destinationX, out int destinationY, out nint child);
    [DllImport("libXtst.so.6")] private static extern int XTestFakeMotionEvent(nint display, int screen, int x, int y, nuint delay);
    [DllImport("libXtst.so.6")] private static extern int XTestFakeButtonEvent(nint display, uint button, int pressed, nuint delay);
    [StructLayout(LayoutKind.Sequential)] private struct XKeyEvent {
        public int Type; public nuint Serial; public int SendEvent; public nint Display, Window, RootWindow, Subwindow;
        public nuint Time; public int X, Y, RootX, RootY; public uint State, Keycode; public int SameScreen;
    }
    [DllImport("libX11.so.6")] private static extern nint XOpenDisplay(nint display);
    [DllImport("libX11.so.6")] private static extern int XCloseDisplay(nint display);
    [DllImport("libX11.so.6")] private static extern byte XKeysymToKeycode(nint display, nuint symbol);
    [DllImport("libX11.so.6")] private static extern int XSendEvent(nint display, nint window, int propagate, nint mask, nint keyEvent);
    [DllImport("libX11.so.6")] private static extern int XFlush(nint display);
    private static void Check(bool condition, string message)
    { if (!condition) throw new InvalidOperationException(message); }
    private static async Task WaitLinuxAsync(Func<bool> ready, string message)
    {
        for (var i = 0; i < 100; i++)
        {
            if (ready()) return;
            await Task.Delay(50);
        }
        throw new InvalidOperationException(message);
    }
    private async Task CaptureLinuxAsync(string directory, string name, FrameworkElement? content = null)
    {
        Root.UpdateLayout();
        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(content ?? Root);
        var pixels = await bitmap.GetPixelsAsync();
        var bytes = new byte[pixels.Length];
        using (var reader = DataReader.FromBuffer(pixels)) reader.ReadBytes(bytes);
        using var output = new SKBitmap(new SKImageInfo(bitmap.PixelWidth, bitmap.PixelHeight, SKColorType.Bgra8888, SKAlphaType.Premul));
        Marshal.Copy(bytes, 0, output.GetPixels(), bytes.Length);
        using var image = SKImage.FromBitmap(output);
        using var png = image.Encode(SKEncodedImageFormat.Png, 100);
        using var file = File.Create(Path.Combine(directory, name));
        png.SaveTo(file);
    }
}
#endif
