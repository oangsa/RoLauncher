using System.Net.Http;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;

namespace RoLauncher.Desktop;

// The same wrapping layout on WinUI and Uno; WrapGrid's layout properties are
// not implemented by the Linux renderer.
public sealed class GameCardPanel : Panel
{
    protected override Windows.Foundation.Size MeasureOverride(Windows.Foundation.Size availableSize)
    {
        foreach (var child in Children) child.Measure(availableSize);
        return Layout(availableSize.Width, false);
    }
    protected override Windows.Foundation.Size ArrangeOverride(Windows.Foundation.Size finalSize)
    {
        Layout(finalSize.Width, true); return finalSize;
    }
    private Windows.Foundation.Size Layout(double width, bool arrange)
    {
        double x = 0, y = 0, rowHeight = 0, usedWidth = 0;
        foreach (var child in Children)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > width) { x = 0; y += rowHeight; rowHeight = 0; }
            if (arrange) child.Arrange(new Windows.Foundation.Rect(x, y, size.Width, size.Height));
            x += size.Width; usedWidth = Math.Max(usedWidth, x); rowHeight = Math.Max(rowHeight, size.Height);
        }
        return new Windows.Foundation.Size(usedWidth, y + rowHeight);
    }
}

public sealed partial class MainWindow
{
    private GameProfile[] _gameProfiles = [];
    private readonly List<(TextBlock Count, ProgressBar Bar)> _statusMeters = [];
    private string _gamesFingerprint = "";
    private ContentDialog? _gameDialog, _changelogDialog;
    private bool _closeToTray = true, _windowSettingsReady, _syncingWindowSettings;

    private AccountRow? ResolveAccount(object sender)
    {
        if (sender is FrameworkElement element)
        {
            if (element.Tag is string id && _rows.FirstOrDefault(r => r.Id == id) is { } row) return row;
            for (DependencyObject? current = element; current is not null; current = VisualTreeHelper.GetParent(current))
            {
                if (current is ListViewItem { Content: AccountRow item }) return item;
                if (current is FrameworkElement { DataContext: AccountRow account }) return account;
            }
        }
        FeedbackMessage("Account no longer available. Refresh and try again.", InfoBarSeverity.Warning);
        return null;
    }
    private async void RepairSelected_Click(object sender, RoutedEventArgs e)
    {
        var selected = Selected();
        if (selected.Length != 1) { FeedbackMessage("Select one account to sign in again."); return; }
        await GuardAsync(async () => {
            await _api.SendAsync(HttpMethod.Post, $"accounts/{selected[0].Id}/repair");
            FeedbackMessage($"Sign in to {selected[0].Username}. Account settings will be retained.");
        });
    }
    private readonly Dictionary<ulong, (string? Name, DateTimeOffset RetryAfter)> _gameNames = [];
    private readonly HashSet<ulong> _loadingGameNames = [];
    private void UpdateGameNames(Snapshot snapshot)
    {
        foreach (var profile in snapshot.GameProfiles ?? [])
            if (!string.IsNullOrWhiteSpace(profile.GameName))
                _gameNames[profile.Target.PlaceId] = (profile.GameName, DateTimeOffset.MaxValue);
        foreach (var row in _rows)
        {
            if (row.Account.Target is not { } target) continue;
            if (_gameNames.TryGetValue(target.PlaceId, out var cached)) row.SetGameName(cached.Name);
            if ((!_gameNames.TryGetValue(target.PlaceId, out cached) || cached.RetryAfter <= DateTimeOffset.UtcNow)
                && _loadingGameNames.Count < 4 && _loadingGameNames.Add(target.PlaceId))
                _ = ResolveGameNameAsync(target.PlaceId);
        }
    }
    private async Task ResolveGameNameAsync(ulong placeId)
    {
        string? name = null;
        try
        {
            var details = await _api.GetAsync<JsonElement>($"games/{placeId}");
            if (details.TryGetProperty("name", out var value)) name = value.GetString();
            if (string.IsNullOrWhiteSpace(name)) name = null;
        }
        catch (Exception ex) when (ex is ApiException or HttpRequestException or TaskCanceledException or ObjectDisposedException or JsonException) { }
        finally { _loadingGameNames.Remove(placeId); }
        if (_disposed) return;
        _gameNames[placeId] = (name, name is null ? DateTimeOffset.UtcNow.AddMinutes(5) : DateTimeOffset.MaxValue);
        foreach (var row in _rows.Where(r => r.Account.Target?.PlaceId == placeId)) row.SetGameName(name);
    }
    private void UpdateWorkspace(Snapshot snapshot)
    {
        _closeToTray = snapshot.CloseToTray;
        _syncingWindowSettings = true;
        CloseBehavior.SelectedIndex = _closeToTray ? 0 : 1;
        _syncingWindowSettings = false; _windowSettingsReady = true;
        var profiles = snapshot.GameProfiles ?? [];
        var fingerprint = JsonSerializer.Serialize(profiles);
        if (_gamesFingerprint != fingerprint)
        {
            _gamesFingerprint = fingerprint; _gameProfiles = profiles;
            GameCards.ItemsSource = _gameProfiles;
            GamesEmpty.Visibility = profiles.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        }
        var accounts = snapshot.Accounts;
        var total = accounts.Length;
        var running = accounts.Count(a => a.Status == "running");
        var rejoining = accounts.Count(a => a.Status is "reconnecting" or "backoff");
        var attention = accounts.Where(a => a.Status is "needs_attention" or "unknown")
            .Select(a => _rows.FirstOrDefault(r => r.Id == a.Id) ?? new AccountRow(a)).ToArray();
        var pending = accounts.Count(a => a.Status is "queued" or "launching");
        var stopped = accounts.Count(a => a.Status == "stopped");
        DashboardCounts.Text = $"Live account overview · updated {DateTimeOffset.Now:HH:mm:ss}";
        TotalMetric.Text = total.ToString(); TotalHint.Text = $"{_gameProfiles.Length} saved games";
        RunningMetric.Text = running.ToString(); RunningHint.Text = $"{pending} queued or launching";
        RejoiningMetric.Text = rejoining.ToString(); RejoiningHint.Text = "Reconnect grace or retry wait";
        AttentionMetric.Text = attention.Length.ToString(); AttentionHint.Text = "Paused or unverified status";
        var longest = _rows.OrderByDescending(row => row.LongestSeconds).FirstOrDefault();
        LongestStreakMetric.Text = longest is null ? "—" : AccountRow.FormatDuration(longest.LongestSeconds);
        LongestStreakHint.Text = longest is null || longest.LongestSeconds == 0 ? "No recorded streak yet" : $"{longest.Alias} · @{longest.Account.Username}";
        var covered = accounts.Count(a => a.AutoRecovery && a.Target is not null);
        CoverageMetric.Text = total == 0 ? "—" : $"{covered * 100.0 / total:0}%";
        CoverageBar.Value = total == 0 ? 0 : covered * 100.0 / total;
        CoverageHint.Text = $"{covered} of {total} accounts have automatic rejoin enabled and a destination.";
        DestinationSummary.Text = $"{accounts.Count(a => a.Target is null)} without a destination · {accounts.Count(a => a.PublicFallbackActive)} using public fallback";
        NetworkSummary.Text = snapshot.NetworkSuspended ? "Network unavailable · launches and rejoins waiting" : "Network available";
        RecoveryList.ItemsSource = attention;
        AttentionEmpty.Visibility = attention.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        AttentionSummary.Text = attention.Length == 0 ? "All clear" : $"{attention.Length} accounts · select an account to inspect or edit";
        var statusCounts = new[] { running, pending, rejoining, attention.Length, stopped };
        if (_statusMeters.Count == 0)
        {
            foreach (var label in new[] { "Running", "Queued / launching", "Rejoining", "Needs attention / unknown", "Stopped" })
            {
                var line = new Grid { ColumnSpacing = 12 };
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                line.Children.Add(new TextBlock { Text = label, FontSize = 12 });
                var value = new TextBlock { FontSize = 12, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold };
                Grid.SetColumn(value, 1); line.Children.Add(value);
                var group = new StackPanel { Spacing = 4 }; group.Children.Add(line);
                var bar = new ProgressBar { Height = 4, Foreground = ThemeInk() };
                group.Children.Add(bar); StatusBreakdown.Children.Add(group);
                _statusMeters.Add((value, bar));
            }
        }
        for (var i = 0; i < statusCounts.Length; i++)
        {
            var (value, bar) = _statusMeters[i];
            value.Text = statusCounts[i].ToString();
            var maximum = Math.Max(1, total);
            if (bar.Maximum != maximum) bar.Maximum = maximum;
            if (bar.Value != statusCounts[i]) bar.Value = statusCounts[i];
        }
    }
    private async void CloseBehavior_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_windowSettingsReady || _syncingWindowSettings) return;
        var choice = CloseBehavior.SelectedIndex == 0;
        await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Patch, "settings/window", new { close_to_tray = choice }); _closeToTray = choice; });
        await RefreshAsync();
    }
    private void DialogGame_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (DialogGame.SelectedItem is GameProfile profile)
        {
            PlaceInput.Text = profile.Target.PlaceId.ToString();
            JobInput.Text = profile.Target.JobId ?? ""; PrivateInput.Text = profile.Target.PrivateServerLink ?? "";
            DialogClearTarget.IsChecked = false;
        }
        SetEditorEnabled(!_saving);
    }
    private static bool IsCardControl(DependencyObject? source, object card)
    {
        for (var node = source; node is not null && !ReferenceEquals(node, card); node = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(node))
            if (node is ButtonBase) return true;
        return false;
    }
    private void GameCard_PointerEntered(object sender, PointerRoutedEventArgs e) => AnimateGameCard(sender, true);
    private void GameCard_PointerExited(object sender, PointerRoutedEventArgs e) => AnimateGameCard(sender, false);
    private void AnimateGameCard(object sender, bool hover)
    {
        if (sender is not Border card) return;
        card.BorderBrush = new SolidColorBrush(hover ? (Root.ActualTheme == ElementTheme.Dark ? Windows.UI.Color.FromArgb(255, 212, 212, 216) : Windows.UI.Color.FromArgb(255, 113, 113, 122)) : (Root.ActualTheme == ElementTheme.Dark ? Windows.UI.Color.FromArgb(255, 63, 63, 70) : Windows.UI.Color.FromArgb(255, 228, 228, 231)));
        var transform = card.RenderTransform as TranslateTransform ?? new TranslateTransform();
        card.RenderTransform = transform;
        var animation = new Microsoft.UI.Xaml.Media.Animation.DoubleAnimation
        {
            To = hover ? -4 : 0, Duration = TimeSpan.FromMilliseconds(140), EnableDependentAnimation = true
        };
        Microsoft.UI.Xaml.Media.Animation.Storyboard.SetTarget(animation, transform);
        Microsoft.UI.Xaml.Media.Animation.Storyboard.SetTargetProperty(animation, "Y");
        var storyboard = new Microsoft.UI.Xaml.Media.Animation.Storyboard();
        storyboard.Children.Add(animation); storyboard.Begin();
    }

    private async void GameCard_Tapped(object sender, TappedRoutedEventArgs e)
    {
        if (IsCardControl(e.OriginalSource as DependencyObject, sender)) return;
        if (sender is FrameworkElement { DataContext: GameProfile profile }) { e.Handled = true; await GuardAsync(() => EditGameAsync(profile)); }
    }
    private async void GameCard_KeyDown(object sender, KeyRoutedEventArgs e)
    {
        if (IsCardControl(e.OriginalSource as DependencyObject, sender)) return;
        if (e.Key is Windows.System.VirtualKey.Enter or Windows.System.VirtualKey.Space && sender is FrameworkElement { DataContext: GameProfile profile })
        { e.Handled = true; await GuardAsync(() => EditGameAsync(profile)); }
    }
    private async void AddGame_Click(object sender, RoutedEventArgs e) => await GuardAsync(() => EditGameAsync(null));
    private async void EditGame_Click(object sender, RoutedEventArgs e)
    {
        var profile = _gameProfiles.FirstOrDefault(p => p.Id == ((FrameworkElement)sender).Tag as string);
        if (profile is not null) await GuardAsync(() => EditGameAsync(profile));
    }
    private async void DeleteGame_Click(object sender, RoutedEventArgs e)
    {
        var profile = _gameProfiles.FirstOrDefault(p => p.Id == ((FrameworkElement)sender).Tag as string);
        if (profile is not null && await ConfirmAsync("Delete saved game?", $"Delete {profile.Name}? Existing account destinations stay saved.", "Delete"))
            await GuardAsync(async () => { await _api.SendAsync(HttpMethod.Delete, $"game-profiles/{profile.Id}"); await RefreshAsync(); });
    }
    private async Task EditGameAsync(GameProfile? profile)
    {
        if (_modalOpen) return;
        var name = new TextBox { Name = "GameProfileName", Header = "Profile name", Text = profile?.Name ?? "", MaxLength = 100 };
        var place = new TextBox { Name = "GamePlace", Header = "Place ID", Text = profile?.Target.PlaceId.ToString() ?? "" };
        var job = new TextBox { Header = "Job ID · optional", Text = profile?.Target.JobId ?? "" };
        var link = new TextBox { Header = "Private server link · optional", Text = profile?.Target.PrivateServerLink ?? "", MaxLength = 2048 };
        var error = new InfoBar { IsClosable = false, Severity = InfoBarSeverity.Error };
        var content = new StackPanel { Spacing = 12 };
        content.Children.Add(new TextBlock { Text = "The game name and thumbnail are fetched from Roblox when saved. Choose a public game, a job ID, or a private server link.", TextWrapping = TextWrapping.Wrap });
        foreach (var field in new UIElement[] { name, place, job, link, error }) content.Children.Add(field);
        var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = profile is null ? "Add saved game" : "Edit saved game", Content = new ScrollViewer { Content = content, MaxHeight = 480 }, PrimaryButtonText = "Save game", CloseButtonText = "Cancel", RequestedTheme = Root.ActualTheme };
        ApplyDialogTheme(dialog); _gameDialog = dialog;
        dialog.Closing += (_, args) => args.Cancel = _saving;
        dialog.PrimaryButtonClick += async (_, args) => {
            var draft = new EditorDraft { Place = place.Text, Job = job.Text, Private = link.Text };
            if (string.IsNullOrWhiteSpace(name.Text) || !draft.TryTarget(out var target, out var message))
            { args.Cancel = true; error.Message = "Enter a profile name and a valid destination."; error.IsOpen = true; return; }
            var deferral = args.GetDeferral(); _saving = true; dialog.IsPrimaryButtonEnabled = false; content.IsHitTestVisible = false;
            var saved = false;
            try {
                await GuardAsync(async () => {
                    string gameName; string? thumbnail;
                    if (profile is not null && profile.Target.PlaceId == target!.PlaceId) { gameName = profile.GameName; thumbnail = profile.ThumbnailUrl; }
                    else {
                        var details = await _api.GetAsync<JsonElement>($"games/{target!.PlaceId}");
                        gameName = details.GetProperty("name").GetString()!;
                        thumbnail = details.GetProperty("thumbnail_url").GetString();
                    }
                    await _api.SendAsync(HttpMethod.Post, "game-profiles", new GameProfile(profile?.Id ?? Guid.NewGuid().ToString(), name.Text.Trim(), gameName, thumbnail, target!));
                    saved = true; FeedbackMessage("Game profile saved.", InfoBarSeverity.Success);
                });
                args.Cancel = !saved; error.Message = Feedback.Message; error.IsOpen = !saved;
            }
            finally { _saving = false; dialog.IsPrimaryButtonEnabled = true; content.IsHitTestVisible = true; deferral.Complete(); }
        };
        _modalOpen = true;
        try { await dialog.ShowAsync(); }
        finally { _modalOpen = false; _gameDialog = null; link.Text = ""; }
        await RefreshAsync();
    }
    private async void Changelog_Click(object sender, RoutedEventArgs e) => await GuardAsync(ShowChangelogAsync);
    private async Task ShowChangelogAsync()
    {
        if (_modalOpen) return;
        using var reader = new StreamReader(typeof(MainWindow).Assembly.GetManifestResourceStream("RoLauncher.Changelog")!);
        var text = reader.ReadToEnd();
        var lines = text.Split('\n'); var versions = 0;
        var recent = string.Join("\n", lines.TakeWhile(line => !System.Text.RegularExpressions.Regex.IsMatch(line, @"^\d+\.\d+\.\d+\s") || ++versions <= 2));
        var content = new StackPanel { Spacing = 16 };
        content.Children.Add(new ScrollViewer { MaxHeight = 400, Content = FormatChangelog(recent) });
        content.Children.Add(new HyperlinkButton { Content = "Full changelog on GitHub", NavigateUri = new Uri("https://github.com/oangsa/RoLauncher/blob/main/CHANGELOG.md") });
        var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = "What’s new", Content = content, CloseButtonText = "Done", RequestedTheme = Root.ActualTheme };
        ApplyDialogTheme(dialog); _modalOpen = true; _changelogDialog = dialog;
        try { await dialog.ShowAsync(); } finally { _modalOpen = false; _changelogDialog = null; }
    }
    private void UpdateTrend(Activity[] history)
    {
        var end = DateTimeOffset.UtcNow; var start = end.AddHours(-24);
        var counts = Enumerable.Range(0, 8).Select(i => history.Count(a => a.Timestamp >= start.AddHours(i * 3) && a.Timestamp < start.AddHours((i + 1) * 3))).ToArray();
        var maximum = Math.Max(1, counts.Max()); TrendBars.Children.Clear(); TrendBars.ColumnDefinitions.Clear();
        TrendScope.Text = (HistoryAccount.SelectedItem as ComboBoxItem)?.Tag is string ? "Selected account" : "All accounts";
        for (var i = 0; i < counts.Length; i++) {
            TrendBars.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var column = new StackPanel { HorizontalAlignment = HorizontalAlignment.Stretch, VerticalAlignment = VerticalAlignment.Bottom, Spacing = 4 };
            column.Children.Add(new TextBlock { Text = counts[i].ToString(), HorizontalAlignment = HorizontalAlignment.Center });
            column.Children.Add(new Border { Height = Math.Max(2, counts[i] * 90.0 / maximum), Background = ThemeInk(), CornerRadius = new CornerRadius(3) });
            column.Children.Add(new TextBlock { Text = start.AddHours(i * 3).ToLocalTime().ToString("HH:mm"), FontSize = 11, HorizontalAlignment = HorizontalAlignment.Center });
            Grid.SetColumn(column, i); TrendBars.Children.Add(column);
        }
        TrendSummary.Text = $"{counts.Sum()} recorded events in the last 24 hours · newest interval at right · history retains up to 1,000 events";
    }
}
