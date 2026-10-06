using System.Net.Http;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private string _editorGroup = "", _editorPolicy = "allow_public";
    private string _profileFingerprint = "", _groupFingerprint = "", _historyFingerprint = "";
    private string _activityJson = "";
    private Activity[] _history = [];
    private int _historyPage;
    private BulkForm? _bulkForm;
    private ContentDialog? _bulkDialog;
    private LaunchProfile[] _profiles = [];
    private UpdateView? _update;
    private static int PolicyIndex(string policy) => policy switch { "stay" => 1, "pause" => 2, _ => 0 };
    private static string PolicyValue(int index) => index switch { 1 => "stay", 2 => "pause", _ => "allow_public" };
    private static ComboBox PolicyPicker() => new()
    {
        HorizontalAlignment = HorizontalAlignment.Stretch, SelectedIndex = 0,
        Items = { "Allow public fallback", "Stay on this destination", "Pause and notify" }
    };
    private void UpdateManagement(Snapshot snapshot)
    {
        UpdateWorkspace(snapshot);
        CompatibilityText.Text = snapshot.Compatibility;
        if (!_savingUpdateSettings)
        {
            _syncingUpdateSettings = true;
            IncludeBetaUpdates.IsChecked = snapshot.IncludeBetaUpdates;
            _syncingUpdateSettings = false;
            _updateSettingsReady = true;
        }
        var groups = _rows.Select(r => r.Account.Group).Distinct().OrderBy(g => g).ToArray();
        var fingerprint = JsonSerializer.Serialize(groups);
        if (fingerprint != _groupFingerprint)
        {
            var selected = (GroupFilter.SelectedItem as ComboBoxItem)?.Tag as string;
            GroupFilter.Items.Clear();
            GroupFilter.Items.Add(new ComboBoxItem { Content = "All groups" });
            foreach (var group in groups) GroupFilter.Items.Add(new ComboBoxItem { Content = group.Length == 0 ? "Ungrouped" : group, Tag = group });
            GroupFilter.SelectedItem = GroupFilter.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (i.Tag as string) == selected) ?? GroupFilter.Items[0];
            _groupFingerprint = fingerprint;
        }
        _profiles = snapshot.Profiles ?? [];
        fingerprint = JsonSerializer.Serialize(_profiles);
        if (fingerprint != _profileFingerprint)
        {
            var id = (ProfilePicker.SelectedItem as LaunchProfile)?.Id;
            ProfilePicker.ItemsSource = _profiles;
            ProfilePicker.SelectedItem = _profiles.FirstOrDefault(p => p.Id == id);
            _profileFingerprint = fingerprint;
        }
        var accounts = _rows.Select(r => new { r.Id, r.Alias }).ToArray();
        fingerprint = JsonSerializer.Serialize(accounts);
        if (fingerprint != _historyFingerprint)
        {
            var id = (HistoryAccount.SelectedItem as ComboBoxItem)?.Tag as string;
            HistoryAccount.Items.Clear(); HistoryAccount.Items.Add(new ComboBoxItem { Content = "All accounts" });
            foreach (var row in _rows) HistoryAccount.Items.Add(new ComboBoxItem { Content = $"{row.Alias} · {row.Id}", Tag = row.Id });
            HistoryAccount.SelectedItem = HistoryAccount.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (i.Tag as string) == id) ?? HistoryAccount.Items[0];
            _historyFingerprint = fingerprint;
        }
    }
    private async Task RefreshHistoryAsync()
    {
        var id = (HistoryAccount.SelectedItem as ComboBoxItem)?.Tag as string;
        var history = await _api.GetAsync<Activity[]>(id is null ? "activity" : $"activity?account_id={Uri.EscapeDataString(id)}");
        if (id != ((HistoryAccount.SelectedItem as ComboBoxItem)?.Tag as string)) return;
        history = history.Select(a => a with { AccountLabel = _rows.FirstOrDefault(r => r.Id == a.AccountId) is { } row ? $"{row.Alias} ({row.Username})" : null }).ToArray();
        var json = JsonSerializer.Serialize(history);
        UpdateTrend(history);
        if (json == _activityJson) return;
        _activityJson = json; _history = history; ShowHistoryPage();
    }
    private void ShowHistoryPage()
    {
        _historyPage = Math.Clamp(_historyPage, 0, Math.Max(0, (_history.Length - 1) / 100));
        HistoryList.ItemsSource = _history.Skip(_historyPage * 100).Take(100).ToArray();
        HistoryPageSummary.Text = _history.Length == 0 ? "No activity for this filter" : $"{_historyPage * 100 + 1}–{Math.Min((_historyPage + 1) * 100, _history.Length)} of {_history.Length}";
        HistoryPrevious.IsEnabled = _historyPage > 0; HistoryNext.IsEnabled = (_historyPage + 1) * 100 < _history.Length;
    }
    private void HistoryPage_Click(object sender, RoutedEventArgs e) { _historyPage += int.Parse((string)((Button)sender).Tag); ShowHistoryPage(); }
    private async void History_Changed(object sender, SelectionChangedEventArgs e)
    {
        _historyPage = 0; _activityJson = "";
        if (_api is not null && RecoveryPage.Visibility == Visibility.Visible) await GuardAsync(RefreshHistoryAsync);
    }
    private async Task RefreshBackupsAsync()
    {
        var selected = BackupPicker.SelectedItem as string;
        var names = await _api.GetAsync<string[]>("backups");
        BackupPicker.ItemsSource = names;
        BackupPicker.SelectedItem = names.Contains(selected) ? selected : names.FirstOrDefault();
    }
    private void SelectVisible_Click(object sender, RoutedEventArgs e) => SelectAllRows();
    private void SelectAllSaved_Click(object sender, RoutedEventArgs e) { ClearFilters_Click(sender, e); SelectAllRows(); }
    private async void BulkEdit_Click(object sender, RoutedEventArgs e) => await GuardAsync(() => ShowBulkAsync(PresetsPage.Visibility == Visibility.Visible ? PresetSelected() : Selected()));
    private async void EditAll_Click(object sender, RoutedEventArgs e) => await GuardAsync(() => ShowBulkAsync(_rows.ToArray()));
    private sealed class BulkForm
    {
        public CheckBox AliasApply = new() { Content = "Apply alias / pattern" }, TargetApply = new() { Content = "Apply destination" },
            ClearTarget = new() { Content = "Clear destination" }, RejoinApply = new() { Content = "Apply automatic rejoin" }, Rejoin = new() { Content = "Automatic rejoin" },
            GroupApply = new() { Content = "Apply group" }, FallbackApply = new() { Content = "Apply fallback policy" };
        public TextBox Alias = new() { Header = "Alias · supports {username}, {id}, {index}", MaxLength = 100 },
            Place = new() { Header = "Place ID" }, Job = new() { Header = "Job ID · optional" }, Private = new() { Header = "Private server link · optional", MaxLength = 2048 }, Group = new() { Header = "Group · blank removes membership", MaxLength = 100 };
        public ComboBox Fallback = PolicyPicker();
        public ComboBox Game = new() { Header = "Saved game · optional", HorizontalAlignment = HorizontalAlignment.Stretch, PlaceholderText = "Custom destination" };
        public InfoBar Error = new() { IsClosable = false, Severity = InfoBarSeverity.Error };
        public BulkDraft Draft() => new()
        {
            Fields = new() { Alias = Alias.Text, Place = Place.Text, Job = Job.Text, Private = Private.Text },
            ApplyAlias = AliasApply.IsChecked == true, ApplyTarget = TargetApply.IsChecked == true, ClearTarget = ClearTarget.IsChecked == true,
            ApplyRejoin = RejoinApply.IsChecked == true, Rejoin = Rejoin.IsChecked == true, ApplyGroup = GroupApply.IsChecked == true, Group = Group.Text,
            ApplyFallback = FallbackApply.IsChecked == true, Fallback = PolicyValue(Fallback.SelectedIndex)
        };
    }
    private static string Common(AccountRow[] rows, Func<Account, string> field) => rows.Select(r => field(r.Account)).Distinct().Count() == 1 ? field(rows[0].Account) : "";
    private async Task ShowBulkAsync(AccountRow[] rows)
    {
        if (_modalOpen || rows.Length == 0) { if (rows.Length == 0) FeedbackMessage("Select accounts first."); return; }
        var ids = rows.Select(r => r.Id).ToArray(); // Freeze scope while the dialog is open.
        var form = new BulkForm(); _bulkForm = form;
        form.Game.ItemsSource = _gameProfiles;
        form.Game.SelectionChanged += (_, _) => {
            if (form.Game.SelectedItem is not GameProfile profile) return;
            form.TargetApply.IsChecked = true; form.ClearTarget.IsChecked = false;
            form.Place.Text = profile.Target.PlaceId.ToString(); form.Job.Text = profile.Target.JobId ?? "";
            form.Private.Text = profile.Target.PrivateServerLink ?? "";
        };
        form.Alias.Text = Common(rows, a => a.Alias); form.Alias.PlaceholderText = "Mixed aliases · example: Team {index} ({username})";
        form.Place.Text = Common(rows, a => a.Target?.PlaceId.ToString() ?? ""); form.Place.PlaceholderText = "Mixed or unset destinations";
        form.Job.Text = Common(rows, a => a.Target?.JobId ?? ""); form.Private.Text = Common(rows, a => a.Target?.PrivateServerLink ?? "");
        form.Group.Text = Common(rows, a => a.Group); form.Group.PlaceholderText = "Mixed or ungrouped";
        form.Rejoin.IsThreeState = true;
        form.Rejoin.IsChecked = rows.All(r => r.Account.AutoRecovery) ? true : rows.All(r => !r.Account.AutoRecovery) ? false : null;
        form.Fallback.SelectedIndex = rows.Select(r => r.Account.FallbackPolicy).Distinct().Count() == 1 ? PolicyIndex(rows[0].Account.FallbackPolicy) : -1;
        form.Fallback.PlaceholderText = "Mixed policies · choose one to apply";
        var content = new StackPanel { Spacing = 10 };
        content.Children.Add(new TextBlock { Text = $"{rows.Length} accounts. Only checked Apply settings change. Destination replaces Place ID, Job ID and private link together, on the next launch.", TextWrapping = TextWrapping.Wrap });
        content.Children.Add(new Expander { Header = "Included accounts", HorizontalAlignment = HorizontalAlignment.Stretch, Content = new ScrollViewer { MaxHeight = 160, Content = new TextBlock { Text = string.Join("\n", rows.Select(r => $"{r.Alias} · @{r.Account.Username} · {r.Id}")), TextWrapping = TextWrapping.Wrap } } });
        foreach (var element in new UIElement[] { form.AliasApply, form.Alias, form.TargetApply, form.ClearTarget, form.Game, form.Place, form.Job, form.Private, form.RejoinApply, form.Rejoin, form.GroupApply, form.Group, form.FallbackApply, form.Fallback, form.Error })
        {
            content.Children.Add(element);
            if (element == form.Rejoin)
                content.Children.Add(new TextBlock { Text = "Check Apply automatic rejoin, then choose on or off for all included accounts. Turning it off keeps clients open. Changes apply when you save.", TextWrapping = TextWrapping.Wrap, Style = (Style)Application.Current.Resources["Caption"] });
        }
        void EnableFields()
        {
            form.Alias.IsEnabled = form.AliasApply.IsChecked == true;
            form.ClearTarget.IsEnabled = form.TargetApply.IsChecked == true;
            form.Place.IsEnabled = form.Job.IsEnabled = form.Private.IsEnabled = form.TargetApply.IsChecked == true && form.ClearTarget.IsChecked != true;
            form.Rejoin.IsEnabled = form.RejoinApply.IsChecked == true; form.Group.IsEnabled = form.GroupApply.IsChecked == true;
            form.Fallback.IsEnabled = form.FallbackApply.IsChecked == true;
        }
        foreach (var check in new[] { form.AliasApply, form.TargetApply, form.ClearTarget, form.RejoinApply, form.GroupApply, form.FallbackApply })
        {
            check.Checked += (_, _) => EnableFields(); check.Unchecked += (_, _) => EnableFields();
        }
        EnableFields();
        var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = $"Edit {rows.Length} accounts", Content = new ScrollViewer { Content = content, MaxHeight = 520, VerticalScrollBarVisibility = ScrollBarVisibility.Auto }, PrimaryButtonText = "Apply changes", CloseButtonText = "Cancel", RequestedTheme = Root.ActualTheme };
        ApplyDialogTheme(dialog); _bulkDialog = dialog;
        dialog.Closing += (_, args) => args.Cancel = _saving;
        dialog.PrimaryButtonClick += async (_, args) =>
        {
            args.Cancel = true;
            if (!form.Draft().TryPatch(out var patch, out var error)) { form.Error.Message = error; form.Error.IsOpen = true; return; }
            if (form.RejoinApply.IsChecked == true && form.Rejoin.IsChecked is null || form.FallbackApply.IsChecked == true && form.Fallback.SelectedIndex < 0)
            { form.Error.Message = "Choose a specific rejoin value and fallback policy for the settings being applied."; form.Error.IsOpen = true; return; }
            var deferral = args.GetDeferral(); _saving = true; dialog.IsPrimaryButtonEnabled = false; content.IsHitTestVisible = false;
            try
            {
                var saved = false;
                await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Patch, "accounts/bulk", new { account_ids = ids, patch }); saved = true; });
                args.Cancel = !saved; form.Error.IsOpen = !saved; form.Error.Message = Feedback.Message;
                if (saved) FeedbackMessage($"Settings saved for {ids.Length} accounts.", InfoBarSeverity.Success);
            }
            finally { _saving = false; dialog.IsPrimaryButtonEnabled = true; content.IsHitTestVisible = true; deferral.Complete(); }
        };
        _modalOpen = true;
        try { await dialog.ShowAsync(); }
        finally { _modalOpen = false; _bulkForm = null; _bulkDialog = null; form.Private.Text = ""; }
        await RefreshAsync();
    }
    private async void RetryNow_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).DataContext is AccountRow row) await RunActionAsync("retry", [row]);
    }
    private async void Repair_Click(object sender, RoutedEventArgs e)
    {
        if (((Button)sender).DataContext is AccountRow row) await GuardAsync(async () =>
        {
            await _api.SendAsync(HttpMethod.Post, $"accounts/{Uri.EscapeDataString(row.Id)}/repair");
            FeedbackMessage($"Sign in to @{row.Account.Username}. A different account will be rejected. Settings will be retained.");
        });
    }
    private async void SaveProfile_Click(object sender, RoutedEventArgs e) => await SaveProfileAsync(false);
    private async void ReplaceProfile_Click(object sender, RoutedEventArgs e) => await SaveProfileAsync(true);
    private async Task SaveProfileAsync(bool replace)
    {
        var rows = PresetSelected(); var profile = ProfilePicker.SelectedItem as LaunchProfile;
        if (rows.Length == 0 || replace && profile is null) { FeedbackMessage("Choose accounts in Create a preset and a saved preset when replacing."); return; }
        if (replace && !await ConfirmAsync("Replace preset?", $"Replace {profile!.Name} with the current settings of {rows.Length} chosen accounts?", "Replace")) return;
        await GuardAsync(async () =>
        {
            await _api.SendAsync(HttpMethod.Post, "profiles", new { name = replace && string.IsNullOrWhiteSpace(ProfileName.Text) ? profile!.Name : ProfileName.Text, account_ids = rows.Select(r => r.Id).ToArray(), id = replace ? profile!.Id : null });
            FeedbackMessage("Preset saved.", InfoBarSeverity.Success); await RefreshAsync();
        });
    }
    private async Task<bool> ConfirmAsync(string title, string text, string action)
    {
        if (_modalOpen) return false;
        _modalOpen = true;
        try
        {
            var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = title, Content = new ScrollViewer { Content = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap }, MaxHeight = 450 }, PrimaryButtonText = action, CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close, RequestedTheme = Root.ActualTheme };
            ApplyDialogTheme(dialog); return await dialog.ShowAsync() == ContentDialogResult.Primary;
        }
        finally { _modalOpen = false; }
    }
    private async void LaunchProfile_Click(object sender, RoutedEventArgs e) => await ProfileActionAsync("start");
    private async void ApplyProfile_Click(object sender, RoutedEventArgs e) => await ProfileActionAsync("apply");
    private async Task ProfileActionAsync(string action)
    {
        if (ProfilePicker.SelectedItem is not LaunchProfile profile) { FeedbackMessage("Choose a saved preset first."); return; }
        if (!await ReviewPresetAsync(profile, action)) return;
        await GuardAsync(async () =>
        {
            var result = await _api.SendAsync(HttpMethod.Post, $"profiles/{profile.Id}/{action}");
            if (action == "start")
            {
                var failures = result!.Value.EnumerateArray().Where(r => r.GetProperty("error").ValueKind == JsonValueKind.String).ToArray();
                FeedbackMessage($"Profile queued: {profile.Entries.Length - failures.Length}/{profile.Entries.Length} accounts. {string.Join(" ", failures.Select(r => $"Account {r.GetProperty("account_id").GetString()}: {r.GetProperty("error").GetString()}"))}", failures.Length == 0 ? InfoBarSeverity.Success : InfoBarSeverity.Warning);
            }
            else FeedbackMessage("Profile settings applied.", InfoBarSeverity.Success);
            await RefreshAsync();
        });
    }
    private async void DeleteProfile_Click(object sender, RoutedEventArgs e)
    {
        if (ProfilePicker.SelectedItem is LaunchProfile profile && await ConfirmAsync("Delete profile?", $"Delete {profile.Name}? Its accounts will remain saved.", "Delete"))
            await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Delete, $"profiles/{profile.Id}"); await RefreshAsync(); });
    }
    private async Task ExportJsonAsync(string name, object value)
    {
        var picker = new Windows.Storage.Pickers.FileSavePicker { SuggestedFileName = name };
        picker.FileTypeChoices.Add("JSON", new List<string> { ".json" });
        InitializePicker(picker);
        var file = await picker.PickSaveFileAsync(); if (file is null) return;
        await File.WriteAllTextAsync(file.Path, JsonSerializer.Serialize(value, new JsonSerializerOptions(ApiClient.JsonOptions) { WriteIndented = true }));
        FeedbackMessage("Export saved.", InfoBarSeverity.Success);
    }
    private async void ExportDiagnostics_Click(object sender, RoutedEventArgs e) => await GuardAsync(async () => await ExportJsonAsync("rolauncher-support", await _api.GetAsync<JsonElement>("diagnostics")));
    private async void ExportProfiles_Click(object sender, RoutedEventArgs e)
    {
        if (await ConfirmAsync("Export launch profiles?", "The file includes aliases, account IDs and private-server links. It contains no session cookies, API token or Discord webhook. Keep private links private.", "Export"))
            await GuardAsync(async () => await ExportJsonAsync("rolauncher-profiles", await _api.GetAsync<LaunchProfile[]>("profiles")));
    }
    private async void ImportProfiles_Click(object sender, RoutedEventArgs e) => await GuardAsync(async () =>
    {
        var picker = new Windows.Storage.Pickers.FileOpenPicker(); picker.FileTypeFilter.Add(".json");
        InitializePicker(picker);
        var file = await picker.PickSingleFileAsync(); if (file is null) return;
        if (new FileInfo(file.Path).Length > 1024 * 1024) { FeedbackMessage("Profile file is too large.", InfoBarSeverity.Error); return; }
        LaunchProfile[]? profiles;
        try { profiles = JsonSerializer.Deserialize<LaunchProfile[]>(await File.ReadAllTextAsync(file.Path), ApiClient.JsonOptions); }
        catch (JsonException) { FeedbackMessage("Invalid profile file.", InfoBarSeverity.Error); return; }
        if (profiles is null) { FeedbackMessage("Invalid profile file.", InfoBarSeverity.Error); return; }
        await _api.SendAsync(HttpMethod.Post, "profiles/import", profiles); FeedbackMessage("Profiles imported.", InfoBarSeverity.Success); await RefreshAsync();
    });
    private async void Backup_Click(object sender, RoutedEventArgs e) => await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Post, "backups"); await RefreshBackupsAsync(); FeedbackMessage("Encrypted backup created.", InfoBarSeverity.Success); });
    private async void Restore_Click(object sender, RoutedEventArgs e)
    {
        if (BackupPicker.SelectedItem is not string name) { FeedbackMessage("Choose a backup first."); return; }
        if (!await ConfirmAsync("Restore backup?", "Current accounts, profiles, activity and Discord settings will be replaced. A backup of the current state is created first. All restored accounts remain stopped. Your current API token is retained.", "Restore")) return;
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Post, $"backups/{Uri.EscapeDataString(name)}/restore"); await RefreshAsync(); await RefreshBackupsAsync(); FeedbackMessage("Backup restored. Accounts are stopped.", InfoBarSeverity.Success); });
    }
}
