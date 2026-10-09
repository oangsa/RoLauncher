using System.Collections.ObjectModel;
using System.Net.Http;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Windows.System;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow : Window
{
    private readonly ApiClient _api;
    private readonly Bootstrap _config;
    private readonly ObservableCollection<AccountRow> _rows = [];
    private readonly ObservableCollection<AccountRow> _visibleRows = [];
    private readonly IDisposable _tray;
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromSeconds(1) };
    private readonly DispatcherTimer _feedbackTimer = new() { Interval = TimeSpan.FromSeconds(5) };
    private ContentDialog _accountDialog = null!;
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
        // Linux detaches inline dialogs; keep them alive for subsequent opens.
        _accountDialog = AccountDialog;
        _presetLookupDialog = PresetLookupDialog;
        _api = new(config);
        _config = config;
        Title = $"RoLauncher {config.Version}";
        VersionLabel.Text = $"v{config.Version}";
        AccountList.ItemsSource = _visibleRows;
        PresetAccounts.ItemsSource = _presetVisibleRows;
        InitializeTheme();
        InitializeNavigation();
        Root.AddHandler(UIElement.PointerPressedEvent, new PointerEventHandler(Workspace_PointerPressed), true);

        _tray = ConfigureDesktop();
        Closed += (_, _) => DisposeResources();
        _timer.Tick += async (_, _) => await RefreshAsync();
        _feedbackTimer.Tick += (_, _) => { _feedbackTimer.Stop(); Feedback.IsOpen = false; };
        _timer.Start();
        _ = RefreshAsync();
#if UI_SMOKE
        Root.Loaded += async (sender, e) => await SmokeAsync();
#elif LINUX_UI_SMOKE
        Root.Loaded += async (sender, e) => await LinuxSmokeAsync();
#else
        Root.Loaded += (_, _) => StartUpdateChecks();
#endif
    }

    private AccountRow[] Selected() => _visibleRows.Where(r => r.IsSelected).ToArray();
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
            ApplyPresetFilters();
            UpdateGameNames(snapshot);
            if (RecoveryPage.Visibility == Visibility.Visible) await RefreshHistoryAsync();
            if (BackupsPage.Visibility == Visibility.Visible) await RefreshBackupsAsync();
            if (_editorRow is not null && !_rows.Contains(_editorRow))
            {
                _accountDialog.IsPrimaryButtonEnabled = false;
                EditorError.Message = "This account was removed. Close this dialog to continue.";
                EditorError.IsOpen = true;
            }
            if (snapshot.NetworkSuspended && !_networkSuspended) FeedbackMessage("Connectivity unavailable. New launches and automatic rejoins are waiting.", InfoBarSeverity.Warning);
            _networkSuspended = snapshot.NetworkSuspended;
            var discord = await _api.GetAsync<DiscordView>("settings/discord");
            _discordReady = true;
            DiscordEnabled.IsChecked = discord.Enabled;
            DiscordRecovery.IsChecked = discord.NotifyRecovery;
            if (discord.Bot is { } bot)
            {
                BotStatus.Text = bot.Status;
                BotConnection.Text = bot.ConnectionState switch { "online" => "Online", "starting" => "Starting", _ => "Offline" };
                BotConnection.Foreground = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(255,
                    bot.ConnectionState == "online" ? (byte)46 : bot.ConnectionState == "starting" ? (byte)202 : (byte)220,
                    bot.ConnectionState == "online" ? (byte)160 : bot.ConnectionState == "starting" ? (byte)155 : (byte)60,
                    bot.ConnectionState == "online" ? (byte)67 : bot.ConnectionState == "starting" ? (byte)30 : (byte)60));
                BotToken.PlaceholderText = bot.Configured ? "Token saved" : "Paste a bot token";
                if (!_botLoaded) { BotEnabled.IsChecked = bot.Enabled; BotGuild.Text = bot.GuildId; BotUsers.Text = string.Join(", ", bot.AllowedUsers); _botLoaded = true; }
            }
            DiscordStatus.Text = $"{(discord.Configured ? "Webhook saved" : "No webhook configured")} · {discord.DeliveryStatus}";
            // The API returns only the configured flag; never put the saved secret into the UI.
            WebhookInput.PlaceholderText = discord.Configured ? "●●●●●●●●●●●●●●●●" : "Paste a Discord webhook URL";
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
                row.IsSelected = false;
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
    private async void AccountList_ItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is AccountRow row) await GuardAsync(() => ShowAccountAsync(row));
    }
    private void AccountCheckbox_Click(object sender, RoutedEventArgs e) { if (sender is CheckBox { DataContext: AccountRow row } box) row.IsSelected = box.IsChecked == true; UpdateSelectionSettings(); }
    private void SelectAccounts_Click(object sender, RoutedEventArgs e)
    {
        var select = SelectAccounts.IsChecked == true;
        foreach (var row in _visibleRows) row.IsSelected = select;
        UpdateSelectionSettings();
    }
    private void UpdateSelectionSettings()
    {
        var selected = Selected();
        SelectAccounts.IsChecked = _visibleRows.Count > 0 && selected.Length == _visibleRows.Count;
        StartButton.IsEnabled = StopButton.IsEnabled = RestartButton.IsEnabled = RemoveButton.IsEnabled = selected.Length != 0;
        BulkEditButton.IsEnabled = selected.Length != 0;
        AccountCount.Text = $"{_visibleRows.Count} of {_rows.Count} accounts · {selected.Length} selected · Ctrl+A to select visible";
        UpdatePresetControls();
    }
    private void Workspace_PointerPressed(object sender, PointerRoutedEventArgs e)
    {
        if (e.GetCurrentPoint(Root).Properties.IsLeftButtonPressed && e.OriginalSource is DependencyObject source)
            ClearSelectionFrom(source);
    }
    private void ClearSelectionFrom(DependencyObject source)
    {
        if (_modalOpen) return;
        for (var node = source; node is not null; node = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(node))
        {
            if (node is ListViewItem or ButtonBase or TextBox or PasswordBox or ComboBox) return;
            if (node == Root) break;
        }
        foreach (var row in _rows) row.IsSelected = false;
        UpdateSelectionSettings();
    }
    private async void RowAction_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { DataContext: AccountRow row, Tag: string action } button)
        {
            button.IsEnabled = false;
            try { await RunActionAsync(action, [row]); }
            finally { button.IsEnabled = true; }
        }
    }
    private async void RowDetails_Click(object sender, RoutedEventArgs e)
    {
        if (ResolveAccount(sender) is AccountRow row) await GuardAsync(() => ShowAccountAsync(row));
    }
    private async Task ShowAccountAsync(AccountRow row)
    {
        if (_modalOpen || !_rows.Contains(row)) return;
        _modalOpen = true;
        _editorRow = row;
        _editorOriginal = EditorDraft.From(row.Account);
        DialogFields.DataContext = row;
        DialogGame.ItemsSource = _gameProfiles.Cast<object>().Prepend("Custom destination").ToArray();
        DialogGame.SelectedItem = null;
        AliasInput.Text = _editorOriginal.Alias; PlaceInput.Text = _editorOriginal.Place;
        JobInput.Text = _editorOriginal.Job; PrivateInput.Text = _editorOriginal.Private;
        DialogGame.SelectedItem = (object?)_gameProfiles.FirstOrDefault(p => p.Target == row.Account.Target) ?? "Custom destination";
        DialogRejoin.IsChecked = row.Account.AutoRecovery;
        DialogGroup.Text = row.Account.Group;
        DialogFallback.SelectedIndex = PolicyIndex(row.Account.FallbackPolicy);
        DialogClearTarget.IsChecked = false;
        SetEditorEnabled(true);
        _editorGroup = row.Account.Group; _editorPolicy = row.Account.FallbackPolicy;
        _editorRecovery = row.Account.AutoRecovery;
        EditorError.IsOpen = false;
        _accountDialog.IsPrimaryButtonEnabled = true;
        _accountDialog.XamlRoot = Root.XamlRoot;
        ApplyDialogTheme(_accountDialog);
        try
        {
            await ShowAccountDialogAsync();
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
        if (DialogGame.SelectedItem is GameProfile profile)
        {
            if (profile.Target != _editorRow.Account.Target) patch["target"] = profile.Target;
        }
        else if (DialogClearTarget.IsChecked == true) patch["clear_target"] = true;
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
        _accountDialog.IsPrimaryButtonEnabled = false;
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
            _accountDialog.IsPrimaryButtonEnabled = _editorRow is not null && _rows.Contains(_editorRow);
        }
    }
    private void SetEditorEnabled(bool enabled)
    {
        AliasInput.IsEnabled = enabled;
        var custom = DialogGame.SelectedItem is not GameProfile;
        PlaceInput.IsEnabled = JobInput.IsEnabled = PrivateInput.IsEnabled = enabled && custom;
        DialogRejoin.IsEnabled = enabled;
        DialogGame.IsEnabled = DialogGroup.IsEnabled = DialogFallback.IsEnabled = enabled;
        DialogClearTarget.IsEnabled = enabled && custom;
    }
    private async void RowDelete_Click(object sender, RoutedEventArgs e)
    {
        if (ResolveAccount(sender) is AccountRow row) await GuardAsync(() => DeleteAccountsAsync([row]));
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
                PrimaryButtonText = "Delete", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close, RequestedTheme = Root.ActualTheme
            };
            ApplyDialogTheme(dialog);
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            await RunActionAsync("remove", accounts);
        }
        finally { _modalOpen = false; }
    }
    private void ApplyDialogTheme(ContentDialog dialog)
    {
        dialog.Style = (Style)Application.Current.Resources["WorkspaceDialogStyle"];
        dialog.RequestedTheme = Root.ActualTheme;
        // Explicit text avoids the native template's baked-in Segoe UI presenter.
        if (dialog.Content is string text)
            dialog.Content = new TextBlock { Text = text, TextWrapping = TextWrapping.Wrap,
                FontFamily = (Microsoft.UI.Xaml.Media.FontFamily)Application.Current.Resources["ContentControlThemeFontFamily"] };
        // Share the shell's rhythm without replacing the native dialog template.
        // Keep per-dialog width limits and scrolling for narrow windows.
        dialog.Resources["OverlayCornerRadius"] = Application.Current.Resources["SurfaceCornerRadius"];
        dialog.Resources["ContentDialogPadding"] = Application.Current.Resources["SurfacePadding"];
        dialog.Resources["ContentDialogTopOverlay"] = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
        dialog.Resources["ContentDialogTitleMargin"] = new Thickness(0, 0, 0, 16);
        dialog.Resources["ContentDialogButtonSpacing"] = 12d;
        // WinUI's default-action visual state forcibly replaces that button's style.
        if (dialog.DefaultButton == ContentDialogButton.Close)
        {
            dialog.DefaultButton = ContentDialogButton.None;
            dialog.Opened += (_, _) => DialogControl<Button>(dialog, "CloseButton")?.Focus(FocusState.Programmatic);
        }
        dialog.PrimaryButtonStyle = (Style)Application.Current.Resources[dialog.PrimaryButtonText is "Delete" or "Remove" ? "DangerButtonStyle" : "PrimaryActionButtonStyle"];
        dialog.CloseButtonStyle = (Style)Application.Current.Resources["ActionButtonStyle"];
        dialog.Opened -= Dialog_Opened;
        dialog.Opened += Dialog_Opened;
        if (dialog.PrimaryButtonText is "Delete" or "Remove")
            dialog.Opened += (_, _) =>
            {
                if (DialogControl<Button>(dialog, "PrimaryButton") is { } button) DangerControl_Loaded(button, new RoutedEventArgs());
            };

    }

    private void Dialog_Opened(ContentDialog dialog, ContentDialogOpenedEventArgs args)
    {
        // Inline XAML dialogs can retain a hidden template tree when WinUI moves
        // their content into a popup. Style the visible footer, not that placeholder.
        var commands = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetOpenPopupsForXamlRoot(dialog.XamlRoot)
            .Select(popup => DialogControl<Grid>(popup.Child, "CommandSpace")).FirstOrDefault(grid => grid is not null)
            ?? DialogControl<Grid>(dialog, "CommandSpace");
        if (commands is null) return;
        // One acrylic layer paints the dialog; the footer uses that same surface.
        commands.Background = new Microsoft.UI.Xaml.Media.SolidColorBrush(Microsoft.UI.Colors.Transparent);
        commands.HorizontalAlignment = HorizontalAlignment.Right;
        foreach (var (name, column) in new[] { ("PrimaryButton", 0), ("SecondaryButton", 2), ("CloseButton", 4) })
        {
            if (DialogControl<Button>(commands, name) is not { } button) continue;
            if (column < commands.ColumnDefinitions.Count && button.Visibility == Visibility.Visible)
                commands.ColumnDefinitions[column].Width = GridLength.Auto;
            button.FontFamily = (Microsoft.UI.Xaml.Media.FontFamily)Application.Current.Resources["ContentControlThemeFontFamily"];
            button.MinWidth = 104;
            button.Height = 40;
            button.Padding = new Thickness(16, 8, 16, 8);
            button.CornerRadius = new CornerRadius(8);
        }
    }

    private void Tab_Click(object sender, RoutedEventArgs e) => SwitchPage((ToggleButton)sender, NavigationAnimationsEnabled());
    private void SelectAll_Invoked(KeyboardAccelerator sender, KeyboardAcceleratorInvokedEventArgs e)
    {
        if (_presetLookupDraftIds is not null && Root.XamlRoot is { } lookupRoot && Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(lookupRoot) is not (TextBox or PasswordBox))
        {
            PresetSelectVisible_Click(PresetAccounts, new RoutedEventArgs()); e.Handled = true; return;
        }
        if (!_modalOpen && Root.XamlRoot is { } xamlRoot && Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(xamlRoot) is not (TextBox or PasswordBox) && AccountsPage.Visibility == Visibility.Visible)
        {
            SelectAllRows(); e.Handled = true;
        }
    }
    private async void Action_Click(object sender, RoutedEventArgs e)
    {
        var action = (string)((FrameworkElement)sender).Tag;
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
            FeedbackMessage($"Applied to {success}/{selected.Length} accounts. {string.Join(" ", errors)}", errors.Count == 0 ? InfoBarSeverity.Success : InfoBarSeverity.Warning);
            await RefreshAsync();
        });
    }
    private async void Import_Click(object sender, RoutedEventArgs e)
    {
        var cookies = CookieBatch.Parse(CookieInput.Text);
        CookieInput.Text = "";
        await ImportAsync(cookies);
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
        InitializePicker(picker);
        picker.FileTypeFilter.Add(".txt");
        await GuardAsync(async () =>
        {
            var file = await picker.PickSingleFileAsync();
            if (file is null) return;
            await ImportAsync(CookieBatch.Parse(await File.ReadAllTextAsync(file.Path)));
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
    private bool _botLoaded;
    private async void BotSave_Click(object sender, RoutedEventArgs e)
    {
        var bot = new Dictionary<string, object?> { ["enabled"] = BotEnabled.IsChecked == true, ["guild_id"] = BotGuild.Text.Trim(), ["allowed_users"] = BotUsers.Text.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries) };
        if (!string.IsNullOrWhiteSpace(BotToken.Password)) bot["token"] = BotToken.Password;
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Patch, "settings/discord", new { bot }); BotToken.Password = ""; _botLoaded = false; FeedbackMessage("Bot settings saved."); });
        await RefreshAsync();
    }
    private async void BotControl_Click(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string action })
        {
            await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Post, $"settings/discord/bot/{action}"); });
            await RefreshAsync();
        }
    }

    private async void BotRemove_Click(object sender, RoutedEventArgs e)
    {
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Patch, "settings/discord", new { bot = new { enabled = false, token = "", guild_id = "", allowed_users = Array.Empty<string>() } }); BotToken.Password = ""; _botLoaded = false; });
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
            var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = "Local API token", Content = content, CloseButtonText = "Done", RequestedTheme = Root.ActualTheme };
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
        DisposeUpdateResources();
    }
}
