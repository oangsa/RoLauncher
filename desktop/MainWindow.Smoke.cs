#if UI_SMOKE
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics.Imaging;
using Microsoft.UI.Windowing;
using Microsoft.UI;
using WinRT.Interop;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Media;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private async Task SmokeAsync()
    {
        var directory = Environment.GetEnvironmentVariable("ROLAUNCHER_UI_SMOKE_DIR")!;
        Directory.CreateDirectory(directory);
        try
        {
            if (Environment.GetEnvironmentVariable("ROLAUNCHER_UI_BRIDGE_SMOKE") == "1")
            {
                for (var i = 0; i < 100 && _refreshing; i++) await Task.Delay(50);
                var snapshot = await _api.GetAsync<Snapshot>("status");
                Assert(snapshot.Accounts.Length == 0, "Bridge fixture has no real accounts.");
                Assert(_config.Version == GetType().Assembly.GetName().Version!.ToString(3), "Runtime version comes from Rust and matches the shell release.");
                File.WriteAllText(Path.Combine(directory, "result.txt"), "WinUI bridge passed: inherited pipe bootstrap, authenticated Rust API and clean desktop exit.");
                _exiting = true;
                Close();
                return;
            }
            for (var i = 0; i < 100 && _rows.Count == 0; i++) await Task.Delay(100);
            Assert(_rows.Count == 3, "Three simulated accounts load.");
            _timer.Stop();
            Assert(!Feedback.IsOpen, "No permanent Ready banner is shown at startup.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            var pageHeight = SettingsPage.ActualHeight;
            WebhookTest_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            Assert(Feedback.IsOpen && Feedback.Message == "Test queued. Check recent activity for delivery status.", "Send test displays the toast.");
            var toastPosition = Feedback.TransformToVisual(Root).TransformPoint(new Windows.Foundation.Point());
            Assert(toastPosition.Y == 12 && Math.Abs(toastPosition.X + Feedback.ActualWidth / 2 - Root.ActualWidth / 2) < 1,
                "Toast overlays the top center of the window.");
            Assert(SettingsPage.ActualHeight == pageHeight, "Toast does not move or resize page content.");
            await CaptureAsync(directory, "toast.png");
            await Task.Delay(3000);
            WebhookTest_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(2500);
            Assert(Feedback.IsOpen, "A repeated user action receives a fresh five-second timeout.");
            await Task.Delay(2800);
            Assert(!Feedback.IsOpen && !_feedbackTimer.IsEnabled, "Toast disappears automatically and stops its timer.");
            await CaptureAsync(directory, "toast-dismissed.png");
            FeedbackMessage("Webhook settings saved.", InfoBarSeverity.Success);
            var closeToast = FindVisual<Button>(Feedback, "CloseButton") ?? throw new InvalidOperationException("Toast close button missing.");
            ((IInvokeProvider)new ButtonAutomationPeer(closeToast).GetPattern(PatternInterface.Invoke)).Invoke();
            await Task.Delay(150);
            Assert(!Feedback.IsOpen && !_feedbackTimer.IsEnabled, "Close button dismisses the toast and stops its timer.");
            await GuardAsync(() => throw new System.Net.Http.HttpRequestException(), backgroundFeedback: true);
            await Task.Delay(3000);
            await GuardAsync(() => throw new System.Net.Http.HttpRequestException(), backgroundFeedback: true);
            await Task.Delay(2300);
            Assert(!Feedback.IsOpen, "Repeated background failures do not keep a toast open.");
            _lastRefreshFeedback = null;
            Tab_Click(AccountsTab, new RoutedEventArgs());
            AccountList.SelectedItems.Add(_rows[0]);
            NameSearch.Text = "@SAMPLE_1";
            await Task.Delay(150);
            Assert(_visibleRows.Count == 1 && _visibleRows[0].Id == "1" && Selected().Length == 0, "Username search deselects hidden accounts.");
            AccountList.SelectAll();
            await RunActionAsync("stop", Selected());
            var commands = await _api.GetAsync<string[]>("test/commands");
            Assert(commands.Count(c => c.EndsWith("/stop")) == 1 && commands.Contains("/v1/accounts/1/stop"), "Filtered bulk action touches only visible selection.");
            ClearFilters_Click(NameSearch, new RoutedEventArgs());
            await Task.Delay(150);
            Assert(Selected().Length == 1, "Clearing filters does not reselect hidden rows.");
            StatusFilter.SelectedIndex = 1;
            await Task.Delay(150);
            Assert(_visibleRows.Count == 1 && _visibleRows[0].Id == "0", "Status filter shows running accounts.");
            RejoinFilter.SelectedIndex = 2;
            await Task.Delay(150);
            Assert(_visibleRows.Count == 0 && EmptyTitle.Text == "No matching accounts", "Combined filters show a distinct no-results state.");
            await CaptureAsync(directory, "no-results.png");
            ClearFilters_Click(NameSearch, new RoutedEventArgs());
            await Task.Delay(150);

            var modal = ShowAccountAsync(_rows[0]);
            await Task.Delay(500);
            Assert(_modalOpen && _editorRow!.Id == "0", "Row details open a modal independent of selection.");
            AliasInput.Text = "Studio account";
            await CaptureAsync(directory, "account-modal.png", AccountDialog);
            PlaceInput.Text = "incomplete";
            InvokeDialogButton("PrimaryButton");
            await Task.Delay(500);
            var state = await _api.GetAsync<Snapshot>("status");
            Assert(_modalOpen && EditorError.IsOpen && state.Accounts[0].Alias == "Main account" && state.Accounts[0].Target!.PlaceId == 1818,
                "Invalid target keeps modal open and persists no partial edits.");
            await RefreshAsync();
            Assert(PlaceInput.Text == "incomplete", "Status refresh preserves modal input.");
            PlaceInput.Text = "1818";
            InvokeDialogButton("PrimaryButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts[0].Alias == "Studio account", "Modal Save persists through the actual primary button.");

            modal = ShowAccountAsync(_rows[0]);
            await Task.Delay(500);
            AliasInput.Text = "Cancelled";
            InvokeDialogButton("CloseButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts[0].Alias == "Studio account", "Cancel discards edits.");
            modal = ShowAccountAsync(_rows[0]);
            await Task.Delay(500);
            AliasInput.Text = "REJECT";
            InvokeDialogButton("PrimaryButton");
            await Task.Delay(500);
            Assert(_modalOpen && EditorError.IsOpen && AliasInput.Text == "REJECT", "API rejection stays in modal and retains input.");
            InvokeDialogButton("CloseButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));

            NameSearch.Text = "Studio";
            await Task.Delay(150);
            AccountList.SelectedItems.Add(_rows[0]);
            await RefreshAsync();
            Assert(Selected().Length == 1 && _visibleRows.Count == 1, "Refresh preserves matching selection.");
            modal = ShowAccountAsync(_rows[0]);
            await Task.Delay(500);
            AliasInput.Text = "Workspace account";
            InvokeDialogButton("PrimaryButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_visibleRows.Count == 0 && Selected().Length == 0, "Rename reapplies search and deselects newly hidden row.");
            ClearFilters_Click(NameSearch, new RoutedEventArgs());
            modal = ShowAccountAsync(_rows[0]);
            await Task.Delay(500);
            AliasInput.Text = "Studio account";
            PlaceInput.Text = "1818";
            InvokeDialogButton("PrimaryButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            await CaptureAsync(directory, "accounts.png");
            AccountList.SelectAll();
            Assert(Selected().Length == 3, "Bulk selection selects visible accounts.");
            Assert(RejoinCheck.IsChecked is null, "Mixed rejoin selection is shown.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(500);
            await CaptureAsync(directory, "settings.png");
            RejoinCheck.IsChecked = false;
            Rejoin_Click(RejoinCheck, new RoutedEventArgs());
            await Task.Delay(350);
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts.All(a => !a.AutoRecovery), "Bulk rejoin changes every selected account.");
            Action_Click(StopButton, new RoutedEventArgs());
            await Task.Delay(350);
            commands = await _api.GetAsync<string[]>("test/commands");
            Assert(commands.Count(c => c.EndsWith("/stop")) == 4, "Stop applies to every selected account.");
            Login_Click(LoginButton, new RoutedEventArgs());
            await Task.Delay(150);
            commands = await _api.GetAsync<string[]>("test/commands");
            Assert(commands.Contains("/v1/login"), "Browser sign-in uses the authenticated bridge.");
            Tab_Click(ChangelogTab, new RoutedEventArgs());
            await Task.Delay(500);
            await CaptureAsync(directory, "changelog.png");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var bulk = ShowBulkAsync([_rows[0], _rows[2]]);
            await Task.Delay(450);
            Assert(_bulkForm is not null && _bulkDialog is not null, "Selected-account settings dialog opens.");
            _bulkForm!.GroupApply.IsChecked = true; _bulkForm.Group.Text = "Cancelled group";
            InvokeDialogButton("CloseButton"); await bulk.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.All(r => r.Account.Group == ""), "Bulk Cancel preserves every account.");
            bulk = ShowBulkAsync([_rows[0], _rows[2]]);
            await Task.Delay(450);
            var form = _bulkForm!;
            form.GroupApply.IsChecked = true; form.Group.Text = "Crew";
            form.TargetApply.IsChecked = true; form.Place.Text = "unfinished";
            InvokeDialogButton("PrimaryButton"); await Task.Delay(250);
            Assert(_modalOpen && form.Error.IsOpen && _rows.All(r => r.Account.Group == ""), "Invalid bulk destination prevents partial group saves.");
            form.TargetApply.IsChecked = false; form.AliasApply.IsChecked = true; form.Alias.Text = "REJECT";
            InvokeDialogButton("PrimaryButton"); await Task.Delay(250);
            Assert(_modalOpen && form.Error.IsOpen && form.Group.Text == "Crew", "Rejected bulk save preserves the draft.");
            form.Alias.Text = "Crew {index}"; form.Place.Text = "1818"; form.TargetApply.IsChecked = true;
            form.RejoinApply.IsChecked = true; form.Rejoin.IsChecked = true; form.FallbackApply.IsChecked = true; form.Fallback.SelectedIndex = 2;
            await CaptureAsync(directory, "bulk-settings.png", _bulkDialog);
            ((ScrollViewer)_bulkDialog!.Content).ChangeView(null, 10000, null, true);
            await Task.Delay(150); await CaptureAsync(directory, "bulk-settings-policy.png", _bulkDialog);
            InvokeDialogButton("PrimaryButton"); await bulk.WaitAsync(TimeSpan.FromSeconds(5));
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts.Where(a => a.Id is "0" or "2").All(a => a.Group == "Crew" && a.AutoRecovery && a.FallbackPolicy == "pause") && state.Accounts[1].Group == "", "Bulk save applies every selected setting only to the frozen account scope.");
            Assert(state.Accounts[0].Alias == "Crew 1" && state.Accounts[2].Alias == "Crew 2", "Bulk alias patterns expand per account.");
            GroupFilter.SelectedItem = GroupFilter.Items.OfType<ComboBoxItem>().First(i => (i.Tag as string) == "Crew");
            Assert(_visibleRows.Count == 2 && Selected().All(r => r.Id != "1"), "Group filter excludes and deselects other accounts.");
            SelectAllSaved_Click(GroupFilter, new RoutedEventArgs()); Assert(Selected().Length == 3, "Select all saved clears filters and includes every account.");
            bulk = ShowBulkAsync(_rows.ToArray()); await Task.Delay(450);
            _bulkForm!.GroupApply.IsChecked = true; _bulkForm.Group.Text = "All accounts";
            InvokeDialogButton("PrimaryButton"); await bulk.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.All(r => r.Account.Group == "All accounts"), "Edit all accounts updates the complete saved account set.");
            ProfileName.Text = "Startup team"; await SaveProfileAsync(false);
            Assert(_profiles.Length == 1 && _profiles[0].Entries.Length == 3, "Save selected creates a profile with per-account settings.");
            ProfilePicker.SelectedItem = _profiles[0];
            await _api.SendAsync(System.Net.Http.HttpMethod.Patch, "accounts/bulk", new { account_ids = _rows.Select(r => r.Id).ToArray(), patch = new { group = "Temporary" } });
            var apply = ProfileActionAsync("apply"); await Task.Delay(450);
            await CaptureAsync(directory, "profile-review.png", OpenDialog());
            InvokeDialogButton("PrimaryButton"); await apply.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.All(r => r.Account.Group == "All accounts"), "Reviewed profile restores each saved configuration.");
            Tab_Click(SettingsTab, new RoutedEventArgs()); await RefreshAsync();
            Backup_Click(BackupPicker, new RoutedEventArgs()); await Task.Delay(350);
            Assert(BackupPicker.Items.Count > 0, "Create backup refreshes the backup picker.");
            await CaptureAsync(directory, "profiles-settings.png");
            Tab_Click(RecoveryTab, new RoutedEventArgs()); await RefreshAsync();
            Assert(HistoryList.ItemsSource is Activity[] { Length: 1 }, "Persistent history loads on the recovery page.");
            Assert(HistoryAccount.Items.OfType<ComboBoxItem>().First(i => (i.Tag as string) == "0").Content.ToString()!.Contains("Crew 1"), "History account labels refresh when bulk aliases change.");
            await CaptureAsync(directory, "recovery.png");
            HistoryAccount.SelectedItem = HistoryAccount.Items.OfType<ComboBoxItem>().First(i => (i.Tag as string) == "0"); await Task.Delay(250);
            Assert(HistoryList.ItemsSource is Activity[] { Length: 0 }, "History can filter by account.");
            HistoryAccount.SelectedIndex = 0;
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var hwnd = WindowNative.GetWindowHandle(this);
            var scale = NativeTray.Scale(hwnd);
            AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd)).Resize(new Windows.Graphics.SizeInt32((int)(1020 * scale), (int)(780 * scale)));
            await Task.Delay(350);
            await CaptureAsync(directory, "accounts-small.png");
            FeedbackMessage("Test queued. Check recent activity for delivery status.");
            await CaptureAsync(directory, "toast-small.png");
            Assert(Feedback.ActualWidth <= 640 && Feedback.IsOpen, "Toast remains compact in a smaller window.");
            Feedback.IsOpen = false;

            // Cancel then confirm via actual buttons. Row deletion must ignore the other selected rows.
            var deletion = DeleteAccountsAsync([_rows[1]]);
            await Task.Delay(500);
            await CaptureAsync(directory, "delete-modal.png", OpenDialog());
            InvokeDialogButton("CloseButton");
            await deletion.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.Count == 3, "Cancelled deletion makes no changes.");
            deletion = DeleteAccountsAsync([_rows[1]]);
            await Task.Delay(500);
            InvokeDialogButton("PrimaryButton");
            await deletion.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.Count == 2 && _rows.All(r => r.Id != "1"), "Row delete removes only that account, regardless of selection.");
            var rowToRemove = _rows[1];
            modal = ShowAccountAsync(rowToRemove);
            await Task.Delay(500);
            await CaptureAsync(directory, "account-modal-small.png", AccountDialog);
            await _api.SendAsync(System.Net.Http.HttpMethod.Delete, $"accounts/{rowToRemove.Id}");
            await RefreshAsync();
            Assert(!AccountDialog.IsPrimaryButtonEnabled && EditorError.IsOpen, "External removal disables stale modal save.");
            InvokeDialogButton("CloseButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            AccountList.SelectedItems.Clear();
            _rows.Clear();
            ApplyFilters();
            await CaptureAsync(directory, "empty.png");
            File.WriteAllText(Path.Combine(directory, "result.txt"), "WinUI smoke passed: bulk settings Save/Cancel, atomic invalid/rejected saves, alias patterns, selected/all account scopes, groups and filtering, saved profile review/apply, backup picker, recovery/history and account filtering; inherited toast, modal, deletion, selection, bulk-action, sign-in, resize and rendering checks.");
        }
        catch (Exception ex)
        {
            File.WriteAllText(Path.Combine(directory, "result.txt"), $"FAILED: {ex.GetType().Name}: {ex.Message}\n{ex.StackTrace}");
        }
        _exiting = true;
        Close();
    }
    private static void Assert(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
    private static T? FindVisual<T>(DependencyObject root, string? name = null) where T : FrameworkElement
    {
        if (root is T item && (name is null || item.Name == name)) return item;
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
            if (FindVisual<T>(VisualTreeHelper.GetChild(root, i), name) is T match) return match;
        return null;
    }
    private UIElement OpenDialog() => VisualTreeHelper.GetOpenPopupsForXamlRoot(Root.XamlRoot)
        .Select(p => FindVisual<Border>(p.Child, "BackgroundElement")).First(d => d is not null)!;
    private void InvokeDialogButton(string name)
    {
        var commands = FindVisual<Grid>(OpenDialog(), "CommandSpace") ?? throw new InvalidOperationException("Dialog footer missing.");
        var button = FindVisual<Button>(commands, name) ?? throw new InvalidOperationException($"Dialog button missing: {name}");
        ((IInvokeProvider)new ButtonAutomationPeer(button).GetPattern(PatternInterface.Invoke)).Invoke();
    }
    private async Task CaptureAsync(string directory, string name, UIElement? element = null)
    {
        await Task.Delay(150);
        var bitmap = new RenderTargetBitmap();
        if (element is ContentDialog) element = OpenDialog();
        await bitmap.RenderAsync(element ?? Root);
        Assert(bitmap.PixelWidth > 0 && bitmap.PixelHeight > 0, $"Rendered element has no pixels: {element?.GetType().Name} ({(element as FrameworkElement)?.ActualWidth} x {(element as FrameworkElement)?.ActualHeight}).");
        var pixels = (await bitmap.GetPixelsAsync()).ToArray();
        using var stream = File.Create(Path.Combine(directory, name));
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream.AsRandomAccessStream());
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, (uint)bitmap.PixelWidth,
            (uint)bitmap.PixelHeight, 96, 96, pixels);
        await encoder.FlushAsync();
    }
}
#endif
