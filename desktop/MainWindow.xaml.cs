using System.Collections.ObjectModel;
using System.Net.Http;
using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Windows.System;
using WinRT.Interop;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow : Window
{
    private readonly ApiClient _api;
    private readonly Bootstrap _config;
    private readonly ObservableCollection<AccountRow> _rows = [];
    private readonly ObservableCollection<AccountRow> _visibleRows = [];
    private readonly NativeTray _tray;
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly DispatcherTimer _feedbackTimer = new() { Interval = TimeSpan.FromSeconds(5) };
    private AccountRow? _editorRow;
    private EditorDraft? _editorOriginal;
    private bool _modalOpen, _saving, _editorRecovery;
    private bool _updating, _refreshing, _exiting, _disposed;
    private bool _discordReady;
    private bool _networkSuspended;
    private string? _lastRefreshFeedback;
    private string _activityFingerprint = "";

    public MainWindow(Bootstrap config)
    {
        InitializeComponent();
        _api = new(config);
        _config = config;
        Title = $"RoLauncher {config.Version}";
        VersionLabel.Text = $"v{config.Version}";
        ChangelogText.Text = File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "CHANGELOG.txt"));
        AccountList.ItemsSource = _visibleRows;
        RecoveryList.ItemsSource = _rows;
        var hwnd = WindowNative.GetWindowHandle(this);
        var appWindow = AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd));
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
        _tray = new(hwnd, () => { appWindow.Show(); Activate(); }, async () => await ExitAsync());
        Closed += (_, _) => DisposeResources();
        _timer.Tick += async (_, _) => await RefreshAsync();
        _feedbackTimer.Tick += (_, _) => { _feedbackTimer.Stop(); Feedback.IsOpen = false; };
        _timer.Start();
        _ = RefreshAsync();
#if UI_SMOKE
        Root.Loaded += async (sender, e) => await SmokeAsync();
#endif
    }

    private AccountRow[] Selected() => AccountList.SelectedItems.Cast<AccountRow>().Where(_visibleRows.Contains).ToArray();
    private async Task GuardAsync(Func<Task> action, bool backgroundFeedback = false)
    {
        void Report(string message, InfoBarSeverity severity)
        {
            if (!backgroundFeedback || message != _lastRefreshFeedback) FeedbackMessage(message, severity);
            if (backgroundFeedback) _lastRefreshFeedback = message;
        }
        try { await action(); }
        catch (ApiException e) { Report(e.Message, InfoBarSeverity.Error); }
        catch (HttpRequestException) { Report("Cannot reach the local supervisor. Check that RoLauncher is still running.", InfoBarSeverity.Error); }
        catch (TaskCanceledException) { Report("The request timed out. Check account status before trying again.", InfoBarSeverity.Warning); }
        catch (IOException) { Report("Unable to read the selected file.", InfoBarSeverity.Error); }
        catch (UnauthorizedAccessException) { Report("Access to the selected file was denied.", InfoBarSeverity.Error); }
        catch (System.Runtime.InteropServices.COMException) { Report("Windows could not open the dialog. Try again after closing any other open dialog.", InfoBarSeverity.Error); }
    }
    private void FeedbackMessage(string message, InfoBarSeverity severity = InfoBarSeverity.Informational)
    {
        if (_disposed) return;
        _feedbackTimer.Stop();
        Feedback.Message = message;
        Feedback.Severity = severity;
        Feedback.IsOpen = true;
        _feedbackTimer.Start();
    }
    private void Feedback_Closed(InfoBar sender, InfoBarClosedEventArgs args) => _feedbackTimer.Stop();

    private async Task RefreshAsync()
    {
        if (_refreshing || _disposed || _exiting) return;
        _refreshing = true;
        await GuardAsync(async () =>
        {
            var snapshot = await _api.GetAsync<Snapshot>("status");
            if (_disposed) return;
            _updating = true;
            try
            {
                var ids = snapshot.Accounts.Select(a => a.Id).ToHashSet();
                foreach (var row in _rows.Where(r => !ids.Contains(r.Id)).ToArray()) _rows.Remove(row);
                foreach (var account in snapshot.Accounts)
                {
                    var row = _rows.FirstOrDefault(r => r.Id == account.Id);
                    if (row is null) _rows.Add(new(account)); else row.Update(account);
                }
            }
            finally { _updating = false; }
            ApplyFilters();
            UpdateManagement(snapshot);
            if (RecoveryPage.Visibility == Visibility.Visible) await RefreshHistoryAsync();
            if (SettingsPage.Visibility == Visibility.Visible) await RefreshBackupsAsync();
            if (_editorRow is not null && !_rows.Contains(_editorRow))
            {
                AccountDialog.IsPrimaryButtonEnabled = false;
                EditorError.Message = "This account was removed. Close this dialog to continue.";
                EditorError.IsOpen = true;
            }
            if (snapshot.NetworkSuspended && !_networkSuspended) FeedbackMessage("Connectivity unavailable. New launches and automatic rejoins are waiting.", InfoBarSeverity.Warning);
            _networkSuspended = snapshot.NetworkSuspended;
            var discord = await _api.GetAsync<DiscordView>("settings/discord");
            _discordReady = true;
            DiscordEnabled.IsChecked = discord.Enabled;
            DiscordRecovery.IsChecked = discord.NotifyRecovery;
            DiscordStatus.Text = $"{(discord.Configured ? "Webhook saved" : "No webhook configured")} · {discord.DeliveryStatus}";
            var fingerprint = JsonSerializer.Serialize(discord.Recent);
            if (fingerprint != _activityFingerprint)
            {
                ActivityList.ItemsSource = discord.Recent;
                ActivityEmpty.Visibility = discord.Recent.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
                _activityFingerprint = fingerprint;
            }
            _lastRefreshFeedback = null;
        }, backgroundFeedback: true);
        _refreshing = false;
    }

    private void Filters_Changed(object sender, RoutedEventArgs e)
    {
        // XAML raises selection events during InitializeComponent, before every field exists.
        if (AccountList is null || NameSearch is null || StatusFilter is null || RejoinFilter is null || GroupFilter is null || EmptyState is null) return;
        ApplyFilters();
    }
    private void ClearFilters_Click(object sender, RoutedEventArgs e)
    {
        NameSearch.Text = "";
        StatusFilter.SelectedIndex = RejoinFilter.SelectedIndex = 0;
        GroupFilter.SelectedIndex = 0;
        ApplyFilters();
    }
    private void ApplyFilters()
    {
        var status = (StatusFilter.SelectedItem as ComboBoxItem)?.Tag as string ?? "";
        var rejoin = (RejoinFilter.SelectedItem as ComboBoxItem)?.Tag as string ?? "";
        var group = (GroupFilter.SelectedItem as ComboBoxItem)?.Tag as string;
        var matches = _rows.Where(r => AccountFilter.Matches(r.Account, NameSearch.Text, status, rejoin) && (group is null || r.Account.Group == group)).ToArray();
        _updating = true;
        try
        {
            foreach (var row in _visibleRows.Where(r => !matches.Contains(r)).ToArray())
            {
                AccountList.SelectedItems.Remove(row);
                _visibleRows.Remove(row);
            }
            for (var i = 0; i < matches.Length; i++)
                if (!_visibleRows.Contains(matches[i])) _visibleRows.Insert(i, matches[i]);
        }
        finally { _updating = false; }
        EmptyState.Visibility = matches.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        EmptyTitle.Text = _rows.Count == 0 ? "No accounts yet" : "No matching accounts";
        EmptyHint.Text = _rows.Count == 0 ? "Add your first account above to get started." : "Try another name or clear the filters.";
        UpdateSelectionSettings();
    }
    private void AccountList_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (!_updating) UpdateSelectionSettings();
    }
    private void UpdateSelectionSettings()
    {
        var selected = Selected();
        foreach (var button in new[] { StartButton, StopButton, RestartButton, RemoveButton }) button.IsEnabled = selected.Length != 0;
        BulkEditButton.IsEnabled = selected.Length != 0;
        AccountCount.Text = $"{_visibleRows.Count} of {_rows.Count} accounts · {selected.Length} selected · Ctrl+A to select visible";
        var enabled = selected.Count(r => r.Account.AutoRecovery);
        RejoinCheck.IsEnabled = selected.Length != 0;
        RejoinCheck.IsChecked = enabled == 0 ? false : enabled == selected.Length ? true : null;
        SelectionSummary.Text = selected.Length == 0 ? "Select accounts on the Accounts page to change their settings."
            : $"{selected.Length} selected · Automatic rejoin is on for {enabled}.";
    }
    private async void RowDetails_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).DataContext is AccountRow row) await GuardAsync(() => ShowAccountAsync(row));
    }
    private async Task ShowAccountAsync(AccountRow row)
    {
        if (_modalOpen || !_rows.Contains(row)) return;
        _modalOpen = true;
        _editorRow = row;
        _editorOriginal = EditorDraft.From(row.Account);
        DialogFields.DataContext = row;
        AliasInput.Text = _editorOriginal.Alias; PlaceInput.Text = _editorOriginal.Place;
        JobInput.Text = _editorOriginal.Job; PrivateInput.Text = _editorOriginal.Private;
        DialogRejoin.IsChecked = row.Account.AutoRecovery;
        DialogGroup.Text = row.Account.Group;
        DialogFallback.SelectedIndex = PolicyIndex(row.Account.FallbackPolicy);
        DialogClearTarget.IsChecked = false;
        _editorGroup = row.Account.Group; _editorPolicy = row.Account.FallbackPolicy;
        _editorRecovery = row.Account.AutoRecovery;
        EditorError.IsOpen = false;
        AccountDialog.IsPrimaryButtonEnabled = true;
        AccountDialog.XamlRoot = Root.XamlRoot;
        try
        {
            await AccountDialog.ShowAsync();
        }
        finally
        {
            _editorRow = null; _editorOriginal = null; _modalOpen = false;
            DialogFields.DataContext = null;
            AliasInput.Text = PlaceInput.Text = JobInput.Text = PrivateInput.Text = "";
        }
        await RefreshAsync();
    }
    private async void AccountDialog_Save(ContentDialog sender, ContentDialogButtonClickEventArgs args)
    {
        var deferral = args.GetDeferral();
        try { args.Cancel = !await SaveEditorAsync(); }
        finally { deferral.Complete(); }
    }
    private void AccountDialog_Closing(ContentDialog sender, ContentDialogClosingEventArgs args) => args.Cancel = _saving;
    private async Task<bool> SaveEditorAsync()
    {
        if (_saving || _editorRow is null || _editorOriginal is null) return false;
        if (!_rows.Contains(_editorRow))
        { EditorError.Message = "This account was removed. Close this dialog to continue."; EditorError.IsOpen = true; return false; }
        var draft = new EditorDraft { Alias = AliasInput.Text, Place = PlaceInput.Text, Job = JobInput.Text, Private = PrivateInput.Text };
        var patch = new Dictionary<string, object?>();
        if (draft.Alias != _editorOriginal.Alias) patch["alias"] = draft.Alias;
        if (DialogClearTarget.IsChecked == true) patch["clear_target"] = true;
        else if (draft.Place != _editorOriginal.Place || draft.Job != _editorOriginal.Job || draft.Private != _editorOriginal.Private)
        {
            if (!draft.TryTarget(out var target, out var error))
            { EditorError.Message = error; EditorError.IsOpen = true; return false; }
            patch["target"] = target;
        }
        if ((DialogRejoin.IsChecked == true) != _editorRecovery) patch["auto_recovery"] = DialogRejoin.IsChecked == true;
        if (DialogGroup.Text != _editorGroup) patch["group"] = DialogGroup.Text;
        if (PolicyValue(DialogFallback.SelectedIndex) != _editorPolicy) patch["fallback_policy"] = PolicyValue(DialogFallback.SelectedIndex);
        if (patch.Count == 0) return true;
        _saving = true;
        SetEditorEnabled(false);
        AccountDialog.IsPrimaryButtonEnabled = false;
        var saved = false;
        try
        {
            await GuardAsync(async () =>
            {
                await _api.SendAsync(HttpMethod.Patch, $"accounts/{Uri.EscapeDataString(_editorRow.Id)}", patch);
                saved = true;
                FeedbackMessage("Account changes saved.", InfoBarSeverity.Success);
            });
            EditorError.IsOpen = !saved;
            if (!saved) EditorError.Message = Feedback.Message;
            return saved;
        }
        finally
        {
            _saving = false; SetEditorEnabled(true);
            AccountDialog.IsPrimaryButtonEnabled = _editorRow is not null && _rows.Contains(_editorRow);
        }
    }
    private void SetEditorEnabled(bool enabled)
    {
        AliasInput.IsEnabled = PlaceInput.IsEnabled = JobInput.IsEnabled = PrivateInput.IsEnabled = enabled;
        DialogRejoin.IsEnabled = enabled;
        DialogGroup.IsEnabled = DialogFallback.IsEnabled = DialogClearTarget.IsEnabled = enabled;
    }
    private async void RowDelete_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).DataContext is AccountRow row) await GuardAsync(() => DeleteAccountsAsync([row]));
    }
    private async Task DeleteAccountsAsync(AccountRow[] accounts)
    {
        if (_modalOpen || accounts.Length == 0) return;
        _modalOpen = true;
        try
        {
            var dialog = new ContentDialog
            {
                XamlRoot = Root.XamlRoot, Title = accounts.Length == 1 ? "Delete account?" : $"Delete {accounts.Length} accounts?",
                Content = accounts.Length == 1 ? $"Remove {accounts[0].Alias} ({accounts[0].Username}) from RoLauncher? Stop the account before deleting it."
                    : "Remove the selected accounts from RoLauncher? Stop them before deleting. Running accounts cannot be removed.",
                PrimaryButtonText = "Delete", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close, RequestedTheme = ElementTheme.Light
            };
            ApplyDialogTheme(dialog);
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            await RunActionAsync("remove", accounts);
        }
        finally { _modalOpen = false; }
    }
    private void ApplyDialogTheme(ContentDialog dialog)
    {
        // Popup dialogs created in code need the same monochrome aliases as the page.
        foreach (var resource in Root.Resources) dialog.Resources[resource.Key] = resource.Value;
    }

    private void Tab_Click(object sender, RoutedEventArgs e)
    {
        var page = int.Parse((string)((ToggleButton)sender).Tag);
        AccountsTab.IsChecked = page == 0; SettingsTab.IsChecked = page == 1; ChangelogTab.IsChecked = page == 2;
        AccountsPage.Visibility = page == 0 ? Visibility.Visible : Visibility.Collapsed;
        SettingsPage.Visibility = page == 1 ? Visibility.Visible : Visibility.Collapsed;
        ChangelogPage.Visibility = page == 2 ? Visibility.Visible : Visibility.Collapsed;
        RecoveryTab.IsChecked = page == 3;
        RecoveryPage.Visibility = page == 3 ? Visibility.Visible : Visibility.Collapsed;
        if (page == 3) _ = GuardAsync(RefreshHistoryAsync);
    }
    private void Root_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        var control = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control);
        if (e.Key == VirtualKey.A && control.HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down) &&
            !_modalOpen && Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(Root.XamlRoot) is not (TextBox or PasswordBox) && AccountsPage.Visibility == Visibility.Visible)
        {
            AccountList.SelectAll(); e.Handled = true;
        }
    }
    private async void Action_Click(object sender, RoutedEventArgs e)
    {
        var action = (string)((Button)sender).Tag;
        var selected = Selected();
        if (action == "remove") await GuardAsync(() => DeleteAccountsAsync(selected));
        else await RunActionAsync(action, selected);
    }
    private async Task RunActionAsync(string action, AccountRow[] selected)
    {
        if (selected.Length == 0) return;
        await GuardAsync(async () =>
        {
            var errors = new List<string>(); var success = 0;
            foreach (var row in selected)
            {
                try
                {
                    var path = $"accounts/{Uri.EscapeDataString(row.Id)}";
                    await _api.SendAsync(action == "remove" ? HttpMethod.Delete : HttpMethod.Post, action == "remove" ? path : $"{path}/{action}");
                    success++;
                }
                catch (ApiException ex) { errors.Add($"{row.Alias}: {ex.Message}"); }
            }
            FeedbackMessage($"Applied to {success}/{selected.Length} selected accounts. {string.Join(" ", errors)}", errors.Count == 0 ? InfoBarSeverity.Success : InfoBarSeverity.Warning);
            await RefreshAsync();
        });
    }
    private async void Rejoin_Click(object sender, RoutedEventArgs e)
    {
        var enabled = RejoinCheck.IsChecked != false;
        var selected = Selected();
        await GuardAsync(async () =>
        {
            var errors = new List<string>(); var success = 0;
            foreach (var row in selected)
            {
                try { await _api.SendAsync(HttpMethod.Patch, $"accounts/{Uri.EscapeDataString(row.Id)}", new { auto_recovery = enabled }); success++; }
                catch (ApiException ex) { errors.Add($"{row.Alias}: {ex.Message}"); }
            }
            FeedbackMessage($"Rejoin updated for {success}/{selected.Length} accounts. {string.Join(" ", errors)}", errors.Count == 0 ? InfoBarSeverity.Success : InfoBarSeverity.Warning);
        });
        await RefreshAsync();
    }
    private async void Import_Click(object sender, RoutedEventArgs e)
    {
        var cookie = CookieInput.Password;
        CookieInput.Password = "";
        await ImportAsync([cookie]);
    }
    private async Task ImportAsync(string[] cookies)
    {
        ImportButton.IsEnabled = FileButton.IsEnabled = false;
        await GuardAsync(async () =>
        {
            if (cookies.Length == 0 || cookies.All(string.IsNullOrWhiteSpace)) { FeedbackMessage("Paste a session cookie first.", InfoBarSeverity.Warning); return; }
            int success = 0; var errors = new List<string>();
            // One cookie per request keeps each import within the request timeout, even for large files.
            foreach (var batch in cookies.Chunk(1))
            {
                var result = (await _api.SendAsync(HttpMethod.Post, "accounts", new { cookies = batch }))!.Value;
                success += result.GetProperty("accounts").GetArrayLength();
                foreach (var error in result.GetProperty("errors").EnumerateArray()) errors.Add(error.GetProperty("error").GetString() ?? "Import failed.");
            }
            FeedbackMessage($"Imported {success} accounts. {string.Join(" ", errors)}", errors.Count == 0 ? InfoBarSeverity.Success : InfoBarSeverity.Warning);
            await RefreshAsync();
        });
        Array.Clear(cookies);
        ImportButton.IsEnabled = FileButton.IsEnabled = true;
    }
    private async void File_Click(object sender, RoutedEventArgs e)
    {
        var picker = new Windows.Storage.Pickers.FileOpenPicker();
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(this));
        picker.FileTypeFilter.Add(".txt");
        await GuardAsync(async () =>
        {
            var file = await picker.PickSingleFileAsync();
            if (file is null) return;
            var lines = await File.ReadAllLinesAsync(file.Path);
            await ImportAsync(lines.Where(s => !string.IsNullOrWhiteSpace(s)).ToArray());
            Array.Clear(lines);
        });
    }
    private async void Login_Click(object sender, RoutedEventArgs e)
    {
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Post, "login"); FeedbackMessage("Browser sign-in opened. Complete your Roblox login there."); });
    }
    private async void DiscordPreference_Click(object sender, RoutedEventArgs e)
    {
        if (!_discordReady) return;
        await GuardAsync(async () => await _api.SendAsync(HttpMethod.Patch, "settings/discord", new { enabled = DiscordEnabled.IsChecked == true, notify_recovery = DiscordRecovery.IsChecked == true }));
        await RefreshAsync();
    }
    private async void WebhookSave_Click(object sender, RoutedEventArgs e)
    {
        var url = WebhookInput.Password;
        await GuardAsync(async () =>
        {
            var patch = new Dictionary<string, object?>();
            if (!string.IsNullOrWhiteSpace(url)) patch["webhook_url"] = url.Trim();
            await _api.SendAsync(HttpMethod.Patch, "settings/discord", patch);
            WebhookInput.Password = "";
            FeedbackMessage("Webhook settings saved.", InfoBarSeverity.Success);
        });
        await RefreshAsync();
    }
    private async void WebhookTest_Click(object sender, RoutedEventArgs e)
    {
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Post, "settings/discord/test"); FeedbackMessage("Test queued. Check recent activity for delivery status."); });
        await RefreshAsync();
    }
    private async void WebhookRemove_Click(object sender, RoutedEventArgs e)
    {
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Patch, "settings/discord", new { webhook_url = "", enabled = false }); WebhookInput.Password = ""; FeedbackMessage("Webhook removed. Notifications are off."); });
        await RefreshAsync();
    }
    private async void Token_Click(object sender, RoutedEventArgs e)
    {
        if (_modalOpen) return;
        _modalOpen = true;
        var button = (Button)sender;
        button.IsEnabled = false;
        var token = new TextBox { Text = _config.Token, IsReadOnly = true, TextWrapping = TextWrapping.Wrap };
        var content = new StackPanel { Spacing = 12 };
        content.Children.Add(new TextBlock { Text = $"Local API: http://127.0.0.1:{_config.Port}/v1\nKeep this bearer token private.", TextWrapping = TextWrapping.Wrap });
        content.Children.Add(token);
        try
        {
            var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = "Local API token", Content = content, CloseButtonText = "Done", RequestedTheme = ElementTheme.Light };
            ApplyDialogTheme(dialog);
            await GuardAsync(async () => await dialog.ShowAsync());
        }
        finally { token.Text = ""; button.IsEnabled = true; _modalOpen = false; }
    }
    private async void Exit_Click(object sender, RoutedEventArgs e) => await ExitAsync();
    private Task ExitAsync()
    {
        if (_exiting || _saving) return Task.CompletedTask;
        _exiting = true;
        Close();
        return Task.CompletedTask;
    }
    public void SupervisorExited()
    {
        _exiting = true;
        Close();
    }
    private void DisposeResources()
    {
        _disposed = true; _timer.Stop(); _feedbackTimer.Stop(); _tray.Dispose(); _api.Dispose();
    }
}
