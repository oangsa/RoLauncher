#if LINUX_UI_SMOKE
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using SkiaSharp;
using Windows.Storage.Streams;
using System.Runtime.InteropServices;

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
            Check(_config.Version == GetType().Assembly.GetName().Version!.ToString(3), "Version matches the supervisor.");
            await CaptureLinuxAsync(directory, "accounts.png");
            NameSearch.Text = "alpha";
            ApplyFilters();
            Check(_visibleRows.Count == 1, "The shared name filter works.");
            SelectAllRows();
            Check(Selected().Length == 1 && StartButton.IsEnabled, "Selection controls share the Windows behavior.");
            ClearFilters_Click(SettingsTab, new RoutedEventArgs());
            Check(_visibleRows.Count == 3, "Clearing filters restores saved accounts.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            await CaptureLinuxAsync(directory, "settings.png");
            Tab_Click(RecoveryTab, new RoutedEventArgs());
            await Task.Delay(150);
            await CaptureLinuxAsync(directory, "recovery.png");
            Tab_Click(ChangelogTab, new RoutedEventArgs());
            await Task.Delay(150);
            await CaptureLinuxAsync(directory, "changelog.png");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var originalAlias = _rows[0].Alias;
            var showing = ShowAccountAsync(_rows[0]);
            await Task.Delay(250);
            Check(AccountDialog.ActualWidth > 0 && AccountDialog.ActualHeight > 0, "The account dialog opens in a popup.");
            Check(DialogFields.DataContext == _rows[0] && AliasInput.Text == originalAlias, "The shared editor binds the selected account.");
            AliasInput.Text = "Cancelled fixture edit";
            await CaptureLinuxAsync(directory, "account-dialog.png", AccountDialog);
            AccountDialog.Hide();
            await showing;
            Check(!_modalOpen && _rows[0].Alias == originalAlias, "Cancel closes the editor without saving edits.");
            showing = ShowAccountAsync(_rows[0]);
            await Task.Delay(150);
            PlaceInput.Text = "invalid place";
            Check(!await SaveEditorAsync() && EditorError.IsOpen && _modalOpen, "Invalid editor changes keep the dialog open.");
            PlaceInput.Text = "1";
            AliasInput.Text = "Saved Linux fixture";
            Check(await SaveEditorAsync(), "Valid editor changes reach the authenticated API.");
            AccountDialog.Hide();
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
            File.WriteAllText(Path.Combine(directory, "result.txt"), "Linux shell smoke passed. Screenshots still require Windows parity review.");
        }
        catch (Exception e)
        {
            // Never include API tokens, cookies, request bodies or raw process arguments.
            File.WriteAllText(Path.Combine(directory, "result.txt"), "FAILED: " + e.GetType().Name + ": " + e.Message);
        }
        finally { _exiting = true; Close(); }
    }

    private static void Check(bool condition, string message)
    { if (!condition) throw new InvalidOperationException(message); }
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
