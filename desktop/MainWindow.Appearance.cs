using System.Text.RegularExpressions;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private int _selectedPage;
    private ToggleButton? _activeTab;
    private Storyboard? _pageTransition, _selectionTransition;

    private (ToggleButton Tab, FrameworkElement Page, int Id)[] Destinations =>
    [
        (AccountsTab, AccountsPage, 0), (GamesTab, GamesPage, 4),
        (RecoveryTab, RecoveryPage, 3), (PresetsTab, PresetsPage, 6),
        (SettingsTab, SettingsPage, 1), (DiscordTab, DiscordPage, 7),
        (BackupsTab, BackupsPage, 8), (UpdatesTab, UpdatesPage, 9),
        (SupportTab, SupportPage, 10)
    ];

    private void InitializeNavigation()
    {
        _activeTab = AccountsTab;
        Root.Loaded += (_, _) => UpdateNavigationSelection(_activeTab, false);
    }

    private void SidebarNavigation_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (_activeTab is not null && NavigationSelection is not null)
            UpdateNavigationSelection(_activeTab, false);
    }

    private void SwitchPage(ToggleButton tab, bool animate)
    {
        var page = int.Parse((string)tab.Tag);
        // ToggleButton toggles itself off when clicked again. Keep the current
        // destination checked without replaying motion or losing scroll position.
        if (page == _selectedPage) { tab.IsChecked = true; return; }
        var previousY = NavigationSelectionTransform.TranslateY;
        foreach (var destination in Destinations)
        {
            var selected = page == destination.Id;
            destination.Tab.IsChecked = selected;
            destination.Tab.FontWeight = selected ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal;
            destination.Page.Visibility = selected ? Visibility.Visible : Visibility.Collapsed;
        }
        _selectedPage = page;
        _activeTab = tab;
        UpdateSelectionSettings();
        PageContent.MinWidth = 0;
        PageScroll.UpdateLayout();
        PageScroll.ChangeView(0, 0, null, true);
        // Keep queued scroll work tied to the destination which scheduled it.
        DispatcherQueue.TryEnqueue(() => { if (_selectedPage == page) PageScroll.ChangeView(0, 0, null, true); });
        var direction = Math.Sign(tab.TransformToVisual(SidebarNavigation).TransformPoint(new Windows.Foundation.Point()).Y - previousY);
        UpdateNavigationSelection(tab, animate);
        AnimatePage(direction, animate);
        if (page == 3) _ = GuardAsync(RefreshHistoryAsync);
        if (page == 8) _ = GuardAsync(RefreshBackupsAsync);
    }

    private void UpdateNavigationSelection(ToggleButton tab, bool animate)
    {
        if (tab.ActualHeight <= 0) return;
        var from = NavigationSelectionTransform.TranslateY;
        _selectionTransition?.Stop();
        var position = tab.TransformToVisual(SidebarNavigation).TransformPoint(new Windows.Foundation.Point()).Y;
        NavigationSelection.Height = tab.ActualHeight;
        NavigationSelectionTransform.TranslateY = position;
        NavigationSelection.Visibility = Visibility.Visible;
        _selectionTransition = null;
        if (!animate || Math.Abs(from - position) < .5) return;
        _selectionTransition = new Storyboard();
        AddMotion(_selectionTransition, NavigationSelectionTransform, "TranslateY", from, position, 260);
        _selectionTransition.Begin();
    }

    private void AnimatePage(int direction, bool animate)
    {
        _pageTransition?.Stop();
        PageContent.Opacity = 1;
        PageTransitionTransform.TranslateY = 0;
        _pageTransition = null;
        if (!animate) return;
        _pageTransition = new Storyboard();
        AddMotion(_pageTransition, PageContent, "Opacity", .78, 1, 180);
        AddMotion(_pageTransition, PageTransitionTransform, "TranslateY", direction < 0 ? -10 : 10, 0, 240);
        _pageTransition.Begin();
    }

    private static void AddMotion(Storyboard storyboard, DependencyObject target, string property,
        double from, double to, int milliseconds)
    {
        var animation = new DoubleAnimation
        {
            From = from, To = to, Duration = new Duration(TimeSpan.FromMilliseconds(milliseconds)), EnableDependentAnimation = target is CompositeTransform,
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut }
        };
        Storyboard.SetTarget(animation, target);
        Storyboard.SetTargetProperty(animation, property);
        storyboard.Children.Add(animation);
    }

    private readonly System.Collections.ObjectModel.ObservableCollection<AccountRow> _presetVisibleRows = [];
    private readonly HashSet<string> _presetChosenIds = [];
    private HashSet<string>? _presetLookupDraftIds;
    private ContentDialog _presetLookupDialog = null!;
    private bool _filteringPresets;
    private bool _themeReady;
    private string? ThemePath => string.IsNullOrWhiteSpace(_config.DataDirectory) ? null : Path.Combine(_config.DataDirectory, "appearance.txt");
    private void InitializeTheme()
    {
        var preference = "System";
        try { if (ThemePath is { } path && File.Exists(path)) preference = File.ReadAllText(path).Trim(); }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { }
        ThemePicker.SelectedIndex = preference switch { "Light" => 1, "Dark" => 2, _ => 0 };
        SetTheme();
        Root.ActualThemeChanged += (_, _) =>
        {
            foreach (var meter in _statusMeters) meter.Bar.Foreground = ThemeInk();
            _activityFingerprint = "";
            if (_history.Length > 0) UpdateTrend(_history);
        };
        _themeReady = true;
    }
    private void SetTheme() => Root.RequestedTheme = ThemePicker.SelectedIndex switch
    { 1 => ElementTheme.Light, 2 => ElementTheme.Dark, _ => ElementTheme.Default };
    private void Theme_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (!_themeReady) return;
        SetTheme();
        try
        {
            if (ThemePath is { } path)
            {
                Directory.CreateDirectory(Path.GetDirectoryName(path)!);
                File.WriteAllText(path, ThemePicker.SelectedIndex switch { 1 => "Light", 2 => "Dark", _ => "System" });
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        { FeedbackMessage("Theme applied, but could not save the preference.", InfoBarSeverity.Warning); }
    }
    private SolidColorBrush ThemeInk() => new(Root.ActualTheme == ElementTheme.Dark
        ? Microsoft.UI.ColorHelper.FromArgb(255, 212, 212, 216) : Microsoft.UI.ColorHelper.FromArgb(255, 24, 24, 27));
    private void AccountTable_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (AccountTable is not null) AccountTable.Width = Math.Max(AccountTable.MinWidth, e.NewSize.Width);
    }
    private AccountRow[] PresetSelected() => _rows.Where(row => _presetChosenIds.Contains(row.Id)).ToArray();
    private HashSet<string> PresetLookupIds => _presetLookupDraftIds ?? _presetChosenIds;
    private async void ChoosePresetAccounts_Click(object sender, RoutedEventArgs e) => await GuardAsync(ShowPresetLookupAsync);
    private async Task ShowPresetLookupAsync()
    {
        if (_modalOpen) return;
        _modalOpen = true;
        _presetLookupDraftIds = new HashSet<string>(_presetChosenIds);
        try
        {
            _presetLookupDialog.XamlRoot = Root.XamlRoot;
            PresetLookupContent.Width = Math.Max(280, Math.Min(820, Root.ActualWidth - 128));
            PresetLookupScroll.MaxHeight = Math.Max(200, Root.ActualHeight - 220);
            ApplyDialogTheme(_presetLookupDialog);
            PresetClearFilters_Click(_presetLookupDialog, new RoutedEventArgs());
            if (await _presetLookupDialog.ShowAsync() == ContentDialogResult.Primary)
            {
                _presetChosenIds.Clear();
                foreach (var id in _presetLookupDraftIds.Where(id => _rows.Any(row => row.Id == id))) _presetChosenIds.Add(id);
            }
        }
        finally
        {
            _presetLookupDraftIds = null;
            _modalOpen = false;
            ApplyPresetFilters();
            ChoosePresetAccountsButton.Focus(FocusState.Programmatic);
        }
    }
    private void PresetClearFilters_Click(object sender, RoutedEventArgs e)
    {
        PresetSearch.Text = "";
        PresetStatus.SelectedIndex = PresetRejoin.SelectedIndex = 0;
        PresetGroup.SelectedIndex = 0;
        ApplyPresetFilters();
    }
    private void PresetTable_SizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (PresetTable is not null) PresetTable.Width = Math.Max(640, e.NewSize.Width);
    }
    private void PresetSearch_Changed(object sender, TextChangedEventArgs e) => ApplyPresetFilters();
    private void PresetGroup_Changed(object sender, SelectionChangedEventArgs e) => ApplyPresetFilters();
    private void ApplyPresetFilters()
    {
        if (PresetAccounts is null || PresetSearch is null || PresetGroup is null || PresetStatus is null || PresetRejoin is null || PresetLookupSummary is null || _filteringPresets) return;
        _filteringPresets = true;
        try
        {
            _presetChosenIds.RemoveWhere(id => !_rows.Any(row => row.Id == id));
            _presetLookupDraftIds?.RemoveWhere(id => !_rows.Any(row => row.Id == id));
            var group = (PresetGroup.SelectedItem as ComboBoxItem)?.Tag as string;
            var groups = _rows.Select(row => row.Account.Group).Distinct().OrderBy(value => value).ToArray();
            if (!PresetGroup.Items.OfType<ComboBoxItem>().Skip(1).Select(item => (string)item.Tag).SequenceEqual(groups))
            {
                PresetGroup.Items.Clear();
                PresetGroup.Items.Add(new ComboBoxItem { Content = "All groups" });
                foreach (var value in groups) PresetGroup.Items.Add(new ComboBoxItem { Content = value.Length == 0 ? "Ungrouped" : value, Tag = value });
                PresetGroup.SelectedItem = PresetGroup.Items.OfType<ComboBoxItem>().FirstOrDefault(item => (item.Tag as string) == group) ?? PresetGroup.Items[0];
                group = (PresetGroup.SelectedItem as ComboBoxItem)?.Tag as string;
            }
            var status = (PresetStatus.SelectedItem as ComboBoxItem)?.Tag as string ?? "";
            var rejoin = (PresetRejoin.SelectedItem as ComboBoxItem)?.Tag as string ?? "";
            var matches = _rows.Where(row => AccountFilter.Matches(row.Account, PresetSearch.Text, status, rejoin) && (group is null || row.Account.Group == group)).ToArray();
            foreach (var row in _presetVisibleRows.Where(row => !matches.Contains(row)).ToArray()) _presetVisibleRows.Remove(row);
            for (var i = 0; i < matches.Length; i++)
                if (!_presetVisibleRows.Contains(matches[i])) _presetVisibleRows.Insert(i, matches[i]);
            foreach (var row in _presetVisibleRows)
                if (PresetLookupIds.Contains(row.Id))
                {
                    if (!PresetAccounts.SelectedItems.Contains(row)) PresetAccounts.SelectedItems.Add(row);
                }
                else PresetAccounts.SelectedItems.Remove(row);
        }
        finally { _filteringPresets = false; }
        PresetLookupEmpty.Visibility = _presetVisibleRows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        UpdatePresetControls();
    }
    private void PresetSelectVisible_Click(object sender, RoutedEventArgs e)
    {
        foreach (var row in _presetVisibleRows)
            if (!PresetAccounts.SelectedItems.Contains(row)) PresetAccounts.SelectedItems.Add(row);
    }
    private void PresetClear_Click(object sender, RoutedEventArgs e)
    {
        PresetLookupIds.Clear();
        PresetAccounts.SelectedItems.Clear();
        UpdatePresetControls();
    }
    private void LocalAccounts_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_filteringPresets || _presetLookupDraftIds is null) return;
        foreach (var row in e.RemovedItems.Cast<AccountRow>().Where(_presetVisibleRows.Contains)) _presetLookupDraftIds.Remove(row.Id);
        foreach (var row in e.AddedItems.Cast<AccountRow>()) _presetLookupDraftIds.Add(row.Id);
        if (_api is not null && !_updating) UpdateSelectionSettings();
    }
    private void PresetName_Changed(object sender, TextChangedEventArgs e) => UpdatePresetControls();
    private void Profile_Changed(object sender, SelectionChangedEventArgs e) => UpdatePresetControls();
    private void UpdatePresetControls()
    {
        if (PresetAccounts is null || SavePresetButton is null || ProfilePicker is null) return;
        var count = PresetSelected().Length;
        PresetSelectionSummary.Text = $"{count} accounts chosen · saves their current settings";
        var lookupCount = PresetLookupIds.Count;
        var hidden = _rows.Count(row => PresetLookupIds.Contains(row.Id) && !_presetVisibleRows.Contains(row));
        PresetLookupSummary.Text = $"{_presetVisibleRows.Count} of {_rows.Count} shown · {lookupCount} selected" + (hidden > 0 ? $" ({hidden} hidden by filters)" : "");
        PresetEditButton.IsEnabled = count > 0;
        SavePresetButton.IsEnabled = count > 0 && !string.IsNullOrWhiteSpace(ProfileName.Text);
        var profile = ProfilePicker.SelectedItem as LaunchProfile;
        LaunchPresetButton.IsEnabled = ApplyPresetButton.IsEnabled = DeletePresetButton.IsEnabled = profile is not null;
        ReplacePresetButton.IsEnabled = profile is not null && count > 0;
        ProfileSummary.Text = profile is null ? "Choose a saved preset, or create one below."
            : $"{profile.Entries.Length} accounts · {profile.Entries.Count(e => e.AutoRecovery)} with rejoin on\n" + string.Join(" · ", profile.Entries.Select(e => e.Alias));
    }
    private void DangerControl_Loaded(object sender, RoutedEventArgs e)
    {
        if (sender is not Control control) return;
        void SetColors()
        {
            var brush = new SolidColorBrush(Root.ActualTheme == ElementTheme.Dark
                ? Microsoft.UI.ColorHelper.FromArgb(255, 255, 148, 148)
                : Microsoft.UI.ColorHelper.FromArgb(255, 185, 28, 28));
            control.Foreground = brush;
            var prefix = control is MenuFlyoutItem ? "MenuFlyoutItem" : "Button";
            foreach (var suffix in new[] { "Foreground", "ForegroundPointerOver", "ForegroundPressed" })
                control.Resources[prefix + suffix] = brush;
        }
        SetColors();
        control.ActualThemeChanged += (_, _) => SetColors();
    }
    private static T? DialogControl<T>(DependencyObject parent, string name) where T : FrameworkElement
    {
        for (var index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++)
        {
            var child = VisualTreeHelper.GetChild(parent, index);
            if (child is T element && element.Name == name) return element;
            if (DialogControl<T>(child, name) is { } found) return found;
        }
        return null;
    }
    private ContentDialog CreatePresetReview(LaunchProfile profile, string action)
    {
        var content = new StackPanel { Spacing = 12 };
        content.Children.Add(new TextBlock { Text = $"{profile.Entries.Length} accounts · Review saved settings", Style = (Style)Application.Current.Resources["Caption"] });
        var accounts = new StackPanel { Spacing = 10 };
        foreach (var entry in profile.Entries)
        {
            var details = new StackPanel { Spacing = 8 };
            details.Children.Add(new TextBlock { Text = entry.Alias, FontSize = 16, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextWrapping = TextWrapping.Wrap });
            AddDetail("Account", entry.AccountId);
            AddDetail("Destination", entry.Target is null ? "No destination saved" : $"Place {entry.Target.PlaceId} · {(entry.Target.PrivateServerLink is not null ? "Private server" : entry.Target.JobId is not null ? "Specific server" : "Public server")}");
            AddDetail("Auto rejoin", entry.AutoRecovery ? "On" : "Off");
            AddDetail("If unavailable", entry.FallbackPolicy switch { "stay" => "Stay on destination", "pause" => "Pause and notify", _ => "Allow public server" });
            AddDetail("Group", string.IsNullOrWhiteSpace(entry.Group) ? "Ungrouped" : entry.Group);
            accounts.Children.Add(new Border { Child = details, Style = (Style)Application.Current.Resources["Card"], Padding = new Thickness(16) });
            void AddDetail(string label, string value)
            {
                var row = new Grid { ColumnSpacing = 12 };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(110) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                row.Children.Add(new TextBlock { Text = label, Style = (Style)Application.Current.Resources["Caption"], FontSize = 13 });
                var text = new TextBlock { Text = value, TextWrapping = TextWrapping.Wrap, FontSize = 13, IsTextSelectionEnabled = true };
                Grid.SetColumn(text, 1); row.Children.Add(text); details.Children.Add(row);
            }
        }
        content.Children.Add(new ScrollViewer { Content = accounts, MaxHeight = 360, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled });
        content.Children.Add(new TextBlock { Text = "These saved settings will replace the accounts’ current settings. Running clients stay open. Changed destinations take effect on the next launch.", TextWrapping = TextWrapping.Wrap, FontSize = 13 });
        var dialog = new ContentDialog { XamlRoot = Root.XamlRoot, Title = $"Review {profile.Name}", Content = content, PrimaryButtonText = action == "start" ? "Apply and start" : "Apply settings", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close };
        dialog.Resources["ContentDialogMaxWidth"] = 640d;
        ApplyDialogTheme(dialog);
        return dialog;
    }
    private async Task<bool> ReviewPresetAsync(LaunchProfile profile, string action)
    {
        if (_modalOpen) return false;
        _modalOpen = true;
        try { return await CreatePresetReview(profile, action).ShowAsync() == ContentDialogResult.Primary; }
        finally { _modalOpen = false; }
    }
    private static StackPanel FormatChangelog(string text)
    {
        var content = new StackPanel { Spacing = 8 };
        foreach (var raw in text.Split('\n'))
        {
            var line = raw.Trim();
            if (line.Length == 0) continue;
            var version = Regex.IsMatch(line, @"^\d+\.\d+\.\d+\s");
            var section = line is "Added" or "Changed" or "Fixed";
            if (line.StartsWith('-') || line.StartsWith('•'))
            {
                var row = new Grid { ColumnSpacing = 8 };
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                row.Children.Add(new TextBlock { Text = "•", FontSize = 14 });
                var body = new TextBlock { Text = line[1..].Trim(), TextWrapping = TextWrapping.Wrap, FontSize = 14, IsTextSelectionEnabled = true };
                Grid.SetColumn(body, 1); row.Children.Add(body); content.Children.Add(row);
            }
            else content.Children.Add(new TextBlock { Text = line, FontSize = version ? 16 : 14,
                FontWeight = version || section ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
                Margin = new Thickness(0, version && content.Children.Count > 0 ? 16 : section ? 8 : 0, 0, 0), TextWrapping = TextWrapping.Wrap });
        }
        return content;
    }
}
