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
                Assert(_config.Version == GetType().Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyInformationalVersionAttribute), false).Cast<System.Reflection.AssemblyInformationalVersionAttribute>().Single().InformationalVersion.Split('+')[0], "Runtime version comes from Rust and matches the shell release.");
                File.WriteAllText(Path.Combine(directory, "result.txt"), "WinUI bridge passed: inherited pipe bootstrap, authenticated Rust API and clean desktop exit.");
                _exiting = true;
                Close();
                return;
            }
            for (var i = 0; i < 100 && _rows.Count == 0; i++) await Task.Delay(100);
            Assert(_rows.Count == 3, "Three simulated accounts load.");
            _timer.Stop();
            for (var i = 0; i < 100 && _refreshing; i++) await Task.Delay(50);
            Assert(!_refreshing, "Initial account and settings refresh finishes before layout measurements.");
            Root.UpdateLayout();
            foreach (var destination in Destinations)
            {
                SwitchPage(destination.Tab, false);
                Root.UpdateLayout();
                var position = destination.Tab.TransformToVisual(SidebarNavigation).TransformPoint(new Windows.Foundation.Point()).Y;
                Assert(Destinations.Count(d => d.Tab.IsChecked == true) == 1 && Destinations.Count(d => d.Page.Visibility == Visibility.Visible) == 1,
                    "Each destination has one selected tab and one immediately interactive page.");
                Assert(NavigationSelection.Visibility == Visibility.Visible && Math.Abs(NavigationSelectionTransform.TranslateY - position) < .5 &&
                    Math.Abs(NavigationSelection.Height - destination.Tab.ActualHeight) < .5,
                    "The selection pill follows the actual row across all sidebar groups.");
                Assert(_pageTransition is null && _selectionTransition is null && PageContent.Opacity == 1 && PageTransitionTransform.TranslateY == 0,
                    "Reduced-motion navigation settles immediately without leaving a hidden or offset page.");
            }
            SwitchPage(RecoveryTab, false);
            await Task.Delay(80); // Let the destination's deferred scroll reset finish.
            PageScroll.ChangeView(null, 180, null, true); await Task.Delay(80);
            var retainedScroll = PageScroll.VerticalOffset;
            RecoveryTab.IsChecked = false; // A real ToggleButton click toggles itself first.
            Tab_Click(RecoveryTab, new RoutedEventArgs());
            Assert(RecoveryTab.IsChecked == true && retainedScroll > 0 && Math.Abs(PageScroll.VerticalOffset - retainedScroll) < .5,
                "Clicking the current destination retains its selection and scroll position.");
            if (NavigationAnimationsEnabled())
            {
                SwitchPage(AccountsTab, true);
                await Task.Delay(50);
                Assert(PageContent.Opacity >= .78 && PageContent.Opacity < 1 && Math.Abs(PageTransitionTransform.TranslateY) > .01,
                    $"Page fade and translation actually animate before settling (opacity {PageContent.Opacity}, offset {PageTransitionTransform.TranslateY}, state {_pageTransition?.GetCurrentState()}, time {_pageTransition?.GetCurrentTime()}).");
                foreach (var destination in Destinations.Reverse())
                {
                    SwitchPage(destination.Tab, true); await Task.Delay(16);
                }
                await Task.Delay(350);
                var position = AccountsTab.TransformToVisual(SidebarNavigation).TransformPoint(new Windows.Foundation.Point()).Y;
                Assert(AccountsPage.Visibility == Visibility.Visible && AccountsTab.IsChecked == true && Math.Abs(PageContent.Opacity - 1) < .001 &&
                    Math.Abs(PageTransitionTransform.TranslateY) < .01 && Math.Abs(NavigationSelectionTransform.TranslateY - position) < .5,
                    "Rapid navigation cancels earlier transitions and settles at the latest selected page.");
            }
            SwitchPage(AccountsTab, false);
            PageScroll.ChangeView(0, 0, null, true);
            var cookiePosition = CookieInput.TransformToVisual(AccountsPage).TransformPoint(new Windows.Foundation.Point());
            var loginPosition = LoginButton.TransformToVisual(AccountsPage).TransformPoint(new Windows.Foundation.Point());
            Assert(loginPosition.Y > cookiePosition.Y && CookieInput.AcceptsReturn && CookieInput.ActualWidth > 200, "Multiline cookie input has a separate action row.");
            CookieInput.Text = " fixture-cookie-a\r\n\nfixture-cookie-b\nfixture-cookie-a ";
            Import_Click(ImportButton, new RoutedEventArgs());
            for (var i = 0; i < 100 && !ImportButton.IsEnabled; i++) await Task.Delay(50);
            Assert(CookieInput.Text.Length == 0 && await _api.GetAsync<int>("test/import-count") == 2, "Cookie paste imports each distinct nonblank line and clears the field.");
            Tab_Click(RecoveryTab, new RoutedEventArgs()); await RefreshAsync();
            Assert(TotalMetric.Text == "3" && RunningMetric.Text == "1" && RejoiningMetric.Text == "1" && AttentionMetric.Text == "1", "Dashboard counts actual account states.");
            Assert(RecoveryList.ItemsSource is AccountRow[] { Length: 1 } attention && attention[0].Id == "2", "Dashboard attention queue excludes healthy accounts and automatic retries.");
            Assert(LongestStreakMetric.Text == "3h 0m 0s" && LongestStreakHint.Text.Contains("Build account"), "Dashboard shows the best saved streak and its account.");
            Assert(_rows[0].RunningSeconds >= 7200 && _rows[1].RunningSeconds >= 3600 && _rows[1].LongestSeconds == 5400, "Account timers continue through retry waits and retain longer historical streaks.");
            Assert(CoverageBar.Value > 66 && CoverageBar.Value < 67, "Coverage requires both automatic rejoin and a destination.");
            var meters = _statusMeters.Select(m => m.Bar).ToArray();
            var statusRows = StatusBreakdown.Children.ToArray();
            await RefreshAsync(); await RefreshAsync();
            Assert(_statusMeters.Select(m => m.Bar).SequenceEqual(meters) && StatusBreakdown.Children.SequenceEqual(statusRows), "Unchanged polls retain status controls instead of restarting their fill animations.");
            Assert(meters[0].Value == 1 && meters[4].Value == 0 && meters.All(m => m.Maximum == 3), "Retained meters reflect the current account distribution.");
            Assert(VisualStateManager.GoToState(PresetsTab, "PointerOver", false), "Sidebar unchecked hover state exists.");
            Assert(VisualStateManager.GoToState(RecoveryTab, "CheckedPointerOver", false), "Sidebar checked hover state exists.");
            Root.UpdateLayout();
            var hoverBorder = FindVisual<ContentPresenter>(PresetsTab, "ContentPresenter");
            var checkedHoverBorder = FindVisual<ContentPresenter>(RecoveryTab, "ContentPresenter");
            Assert(hoverBorder?.Background is SolidColorBrush hoverFill && hoverFill.Color.A > 0 &&
                hoverBorder.BorderBrush is SolidColorBrush hoverStroke && hoverStroke.Color.A == 0 &&
                checkedHoverBorder?.Background is SolidColorBrush checkedFill && checkedFill.Color.A == 0 &&
                checkedHoverBorder.BorderBrush is SolidColorBrush checkedStroke && checkedStroke.Color.A == 0,
                "Unselected sidebar hover has a subtle fill; the selected row preserves the shared pill and transparent border.");
            await CaptureAsync(directory, "sidebar-hover.png");
            VisualStateManager.GoToState(PresetsTab, "Normal", false);
            VisualStateManager.GoToState(RecoveryTab, "Checked", false);
            await CaptureAsync(directory, "dashboard.png");
            UpdateWorkspace(new Snapshot([], true, "fixture"));
            Assert(TotalMetric.Text == "0" && CoverageMetric.Text == "—" && CoverageBar.Value == 0 && AttentionEmpty.Visibility == Visibility.Visible && NetworkSummary.Text.Contains("unavailable"), "Empty dashboard and suspended network have explicit states.");
            Assert(meters.All(m => m.Value == 0 && m.Maximum == 1), "Existing meters update for an empty snapshot.");
            await RefreshAsync();
            Assert(meters[0].Value == 1 && meters[2].Value == 1 && meters[3].Value == 1 && meters.All(m => m.Maximum == 3), "Existing meters update when account counts change.");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            await CheckForUpdateAsync(false);
            Assert(_update?.Available == true && UpdateNotice.IsOpen && UpdateNotice.Visibility == Visibility.Visible,
                "A newer official release appears in the persistent update banner.");
            Assert(UpdateNoticeButton.IsEnabled && (string)UpdateNoticeButton.Content == "Download" && (string)UpdateDownloadButton.Content == "Download",
                "The banner and Settings share the Download action.");
            await CaptureAsync(directory, "update-available.png");
            AssertUpdateButtonLayout();
            _downloadingUpdate = true;
            UpdateActions();
            SetUpdateMessage("Downloading the update… 40%");
            await CaptureAsync(directory, "update-downloading.png");
            AssertUpdateButtonLayout();
            _downloadingUpdate = false;
            UpdateActions();
            Assert(!IncludeBetaUpdates.IsChecked!.Value, "Stable updates are selected by default.");
            IncludeBetaUpdates.IsChecked = true;
            for (var i = 0; i < 100 && (_savingUpdateSettings || _checkingUpdate); i++) await Task.Delay(50);
            Assert(!_savingUpdateSettings && !_checkingUpdate && _update?.Prerelease == true && UpdateNotice.Title.Contains("beta"), "Opting into beta saves the channel and displays a labeled beta update.");
            await RefreshAsync();
            Assert(IncludeBetaUpdates.IsChecked == true, "The saved beta channel survives a settings refresh.");
            await CaptureAsync(directory, "beta-update-available.png");
            IncludeBetaUpdates.IsChecked = false;
            for (var i = 0; i < 100 && (_savingUpdateSettings || _checkingUpdate); i++) await Task.Delay(50);
            Assert(_update?.Prerelease == false, "Turning beta updates off checks the stable channel again.");
            _downloadedUpdate = new("FIXTURE_NOT_EXECUTED", new string('0', 64), _update!.Version);
            UpdateActions();
            Assert((string)UpdateNoticeButton.Content == "Relaunch" && (string)UpdateDownloadButton.Content == "Relaunch",
                "A verified ready installer changes both actions to Relaunch.");
            await CaptureAsync(directory, "update-ready.png");
            AssertUpdateButtonLayout();
            _modalOpen = true;
            await ApplyDownloadedUpdateAsync();
            Assert(!_installingUpdate && !_exiting, "An open editor blocks relaunch without closing the app.");
            _modalOpen = false; Feedback.IsOpen = false;
            _update = null; _downloadedUpdate = null; UpdateNotice.IsOpen = false; UpdateNotice.Visibility = Visibility.Collapsed; UpdateActions();
            Root.UpdateLayout();
            Assert(Math.Abs(GroupFilter.ActualHeight - BulkEditButton.ActualHeight) < 0.5 && GroupFilter.ActualHeight > 0, "The group dropdown and adjacent buttons have equal rendered heights.");
            Assert(!Feedback.IsOpen, "No permanent Ready banner is shown at startup.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            Root.UpdateLayout();
            var pageHeight = SettingsPage.ActualHeight;
            WebhookTest_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(150);
            Assert(Feedback.IsOpen && Feedback.Message == "Test queued. Check recent activity for delivery status.", "Send test displays the toast.");
            var toastPosition = Feedback.TransformToVisual(Root).TransformPoint(new Windows.Foundation.Point());
            Assert(toastPosition.Y == 12 && Math.Abs(toastPosition.X + Feedback.ActualWidth / 2 - (Root.ActualWidth + Root.ColumnDefinitions[0].ActualWidth) / 2) < 1,
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
            _rows[0].IsSelected = true; UpdateSelectionSettings();
            NameSearch.Text = "@SAMPLE_1";
            await Task.Delay(150);
            Assert(_visibleRows.Count == 1 && _visibleRows[0].Id == "1" && Selected().Length == 0, $"Username search deselects hidden accounts. Visible={_visibleRows.Count}, IDs={string.Join(",", _visibleRows.Select(r => r.Id))}, selected={Selected().Length}");
            SelectAllRows();
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
            await CaptureAsync(directory, "account-modal.png", _accountDialog);
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
            _rows[0].IsSelected = true; UpdateSelectionSettings();
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
            SelectAllRows();
            Assert(Selected().Length == 3, "Bulk selection selects visible accounts.");
            SelectAccounts.IsChecked = false; SelectAccounts_Click(SelectAccounts, new RoutedEventArgs());
            Assert(Selected().Length == 0 && _visibleRows.All(r => !r.IsSelected), "Unchecking the header clears every visible account.");
            SelectAccounts.IsChecked = true; SelectAccounts_Click(SelectAccounts, new RoutedEventArgs());
            Assert(Selected().Length == 3 && _visibleRows.All(r => r.IsSelected), "Checking the header selects every visible account despite row callbacks.");
            Tab_Click(SettingsTab, new RoutedEventArgs());
            await Task.Delay(500);
            await CaptureAsync(directory, "settings.png");
            foreach (var tab in new[] { SettingsTab, PresetsTab, BackupsTab, DiscordTab, UpdatesTab, SupportTab })
            {
                Tab_Click(tab, new RoutedEventArgs()); await Task.Delay(150);
                Assert(tab.IsChecked == true && AccountsPage.Visibility == Visibility.Collapsed,
                    "Each sidebar destination selects its own focused page.");
                await CaptureAsync(directory, $"navigation-{tab.Name}.png");
            }
            Assert(SupportPage.Visibility == Visibility.Visible && SettingsPage.Visibility == Visibility.Collapsed,
                "Support and integrations is a dedicated page.");
            Assert(WebhookInput.Password == "" && WebhookInput.PlaceholderText.StartsWith("●"),
                "Saved webhook displays dots without putting a secret or sentinel in the editable value.");
            Tab_Click(DiscordTab, new RoutedEventArgs());
            Assert(BotGuild.Text == "42" && BotUsers.Text == "123" && BotToken.Password == "" && BotConnection.Text == "Offline", "Saved bot settings load without exposing the token.");
            BotSave_Click(BotGuild, new RoutedEventArgs()); await Task.Delay(350);
            Assert(BotConnection.Text == "Offline", "Saving bot settings does not start the bot.");
            foreach (var action in new[] { "start", "stop", "restart" })
            {
                BotControl_Click(new Button { Tag = action }, new RoutedEventArgs()); await Task.Delay(350);
                Assert(BotConnection.Text == (action == "stop" ? "Offline" : "Online"), $"Bot {action} updates its connection status.");
                Assert(BotGuild.Text == "42" && BotUsers.Text == "123" && BotToken.Password == "", $"Bot {action} reuses saved settings without re-entering fields.");
            }
            PageScroll.ChangeView(null, 10000, null, true); await Task.Delay(150);
            await CaptureAsync(directory, "discord-bot-controls.png");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var rejoinEdit = ShowBulkAsync(Selected()); await Task.Delay(350);
            Assert(_bulkForm!.Rejoin.IsChecked is null, "Bulk account settings show mixed rejoin values.");
            _bulkForm.RejoinApply.IsChecked = true; _bulkForm.Rejoin.IsChecked = false;
            InvokeDialogButton("PrimaryButton"); await rejoinEdit.WaitAsync(TimeSpan.FromSeconds(5));
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts.All(a => !a.AutoRecovery), "Bulk account settings disable rejoin for every selected account.");
            Action_Click(StopButton, new RoutedEventArgs());
            await Task.Delay(350);
            commands = await _api.GetAsync<string[]>("test/commands");
            Assert(commands.Count(c => c.StartsWith("/v1/accounts/") && c.EndsWith("/stop")) == 4, "Stop applies to every selected account.");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            await Task.Delay(250);
            Assert(_rows[0].Destination == "Fixture game", "Account destinations resolve a game name without requiring a saved game profile.");
            var rowContainer = (ListViewItem)AccountList.ContainerFromItem(_rows[0]);
            var quickStart = FindVisual<Button>(rowContainer, "RowStartButton")!;
            var quickStop = FindVisual<Button>(rowContainer, "RowStopButton")!;
            var quickRestart = FindVisual<Button>(rowContainer, "RowRestartButton")!;
            Assert(quickStart is not null && quickStop is not null && quickRestart is not null, "Every row exposes three lifecycle quick actions.");
            Root.UpdateLayout();
            var rowDelete = FindVisual<Button>(rowContainer, "RowDetailsButton")!;
            var tableEdge = rowDelete.TransformToVisual(AccountTable).TransformPoint(new Windows.Foundation.Point(rowDelete.ActualWidth, 0));
            Assert(tableEdge.X <= AccountTable.ActualWidth, "Row actions fit inside the account table.");
            AccountTableScroll.ChangeView(AccountTableScroll.ScrollableWidth, null, null, true);
            await Task.Delay(150);
            Root.UpdateLayout();
            var actionEdge = rowDelete.TransformToVisual(AccountTableScroll).TransformPoint(new Windows.Foundation.Point(rowDelete.ActualWidth, 0));
            Assert(actionEdge.X <= AccountTableScroll.ActualWidth + 1, "Horizontal scrolling makes row actions visible in a narrow viewport.");
            AccountTableScroll.ChangeView(0, null, null, true);
            var selectionBefore = Selected().Select(r => r.Id).ToArray();
            ClearSelectionFrom(quickStart!);
            Assert(Selected().Select(r => r.Id).SequenceEqual(selectionBefore), "Clicking a row action preserves bulk selection.");
            foreach (var button in new[] { quickStart!, quickStop!, quickRestart! })
            {
                var before = (await _api.GetAsync<string[]>("test/commands")).Length;
                RowAction_Click(button, new RoutedEventArgs());
                await Task.Delay(350);
                var after = await _api.GetAsync<string[]>("test/commands");
                Assert(after.Skip(before).SequenceEqual(new[] { $"/v1/accounts/0/{button.Tag}" }), "A quick action targets only its own account despite a multi-account selection.");
            }
            ClearSelectionFrom(AccountCount);
            Assert(Selected().Length == 0 && !StartButton.IsEnabled && !StopButton.IsEnabled && !RestartButton.IsEnabled, "Clicking noninteractive workspace space clears selection and disables bulk actions.");
            RowAction_Click(quickStart!, new RoutedEventArgs());
            await Task.Delay(350);
            Assert((await _api.GetAsync<string[]>("test/commands")).Last() == "/v1/accounts/0/start", "Row quick actions work with no bulk selection.");
            SelectVisible_Click(AccountList, new RoutedEventArgs());
            await CaptureAsync(directory, "account-quick-actions.png");
            Login_Click(LoginButton, new RoutedEventArgs());
            await Task.Delay(150);
            commands = await _api.GetAsync<string[]>("test/commands");
            Assert(commands.Contains("/v1/login"), "Browser sign-in uses the authenticated bridge.");
            var changelog = ShowChangelogAsync();
            await Task.Delay(500);
            await CaptureAsync(directory, "changelog.png", OpenDialog());
            InvokeDialogButton("CloseButton"); await changelog;
            Tab_Click(SettingsTab, new RoutedEventArgs());
            PageScroll.ChangeView(null, 10000, null, true); await Task.Delay(150);
            Tab_Click(AccountsTab, new RoutedEventArgs()); await Task.Delay(150);
            Assert(PageScroll.VerticalOffset == 0, "Page switching resets the Settings scroll position.");
            Tab_Click(GamesTab, new RoutedEventArgs());
            var game = EditGameAsync(null); await Task.Delay(300);
            FindVisual<TextBox>(_gameDialog!, "GameProfileName")!.Text = "Private fixture";
            FindVisual<TextBox>(_gameDialog!, "GamePlace")!.Text = "123";
            await CaptureAsync(directory, "game-editor.png", _gameDialog);
            InvokeDialogButton("PrimaryButton"); await game.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_gameProfiles.Length == 1 && _gameProfiles[0].GameName == "Fixture game", "Saving a game resolves metadata and persists its destination.");
            await CaptureAsync(directory, "games.png");
            var gameCard = FindVisual<Border>(GameCards, "SavedGameCard")!;
            AnimateGameCard(gameCard, true); await Task.Delay(200);
            Assert(gameCard.RenderTransform is TranslateTransform lift && lift.Y == -4, "Game card lifts on hover.");
            await CaptureAsync(directory, "games-hover.png");
            AnimateGameCard(gameCard, false); await Task.Delay(200);
            Assert(gameCard.RenderTransform is TranslateTransform rest && rest.Y == 0, "Game card returns to rest when the pointer leaves.");
            game = EditGameAsync(_gameProfiles[0]); await Task.Delay(300);
            FindVisual<TextBox>(_gameDialog!, "GameProfileName")!.Text = "Renamed fixture";
            InvokeDialogButton("PrimaryButton"); await game.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_gameProfiles.Length == 1 && _gameProfiles[0].Name == "Renamed fixture", "Editing preserves game identity.");
            modal = ShowAccountAsync(_rows[0]); await Task.Delay(300);
            DialogGame.SelectedItem = _gameProfiles[0];
            Assert(PlaceInput.Text == "123", "Saved games populate account destinations.");
            Assert(!PlaceInput.IsEnabled && !JobInput.IsEnabled && !PrivateInput.IsEnabled && !DialogClearTarget.IsEnabled,
                "Saved game selection locks duplicate destination controls.");
            await CaptureAsync(directory, "account-saved-game.png", _accountDialog);
            DialogGame.SelectedItem = "Custom destination";
            Assert(PlaceInput.IsEnabled && JobInput.IsEnabled && PrivateInput.IsEnabled && DialogClearTarget.IsEnabled,
                "Explicit Custom destination restores manual editing.");
            DialogGame.SelectedItem = _gameProfiles[0];
            // Saving a preset must use its target, regardless of stale manual field contents.
            PlaceInput.Text = "";
            InvokeDialogButton("PrimaryButton"); await modal.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows[0].Account.Target!.PlaceId == 123, "Account save applies a saved game.");
            modal = ShowAccountAsync(_rows[0]); await Task.Delay(300);
            Assert(DialogGame.SelectedItem is GameProfile && !PlaceInput.IsEnabled,
                "Reopening an account detects its saved destination and keeps it locked.");
            DialogGame.SelectedItem = "Custom destination"; PlaceInput.Text = "456";
            InvokeDialogButton("PrimaryButton"); await modal.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows[0].Account.Target!.PlaceId == 456, "Custom destination saves manually entered target values.");
            CloseBehavior.SelectedIndex = 1; await Task.Delay(300);
            Assert(!_closeToTray && !(await _api.GetAsync<Snapshot>("status")).CloseToTray, "Close preference saves through the API.");
            CloseBehavior.SelectedIndex = 0; await Task.Delay(300);
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
            var teamLookup = ShowPresetLookupAsync(); await Task.Delay(350);
            PresetSelectVisible_Click(PresetAccounts, new RoutedEventArgs());
            InvokeDialogButton("PrimaryButton"); await teamLookup;
            ProfileName.Text = "Startup team"; await SaveProfileAsync(false);
            Assert(_profiles.Length == 1 && _profiles[0].Entries.Length == 3, "Save selected creates a profile with per-account settings.");
            ProfilePicker.SelectedItem = _profiles[0];
            await _api.SendAsync(System.Net.Http.HttpMethod.Patch, "accounts/bulk", new { account_ids = _rows.Select(r => r.Id).ToArray(), patch = new { group = "Temporary" } });
            var apply = ProfileActionAsync("apply"); await Task.Delay(450);
            await CaptureAsync(directory, "profile-review.png", OpenDialog());
            InvokeDialogButton("PrimaryButton"); await apply.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(_rows.All(r => r.Account.Group == "All accounts"), "Reviewed profile restores each saved configuration.");
            Tab_Click(BackupsTab, new RoutedEventArgs()); await RefreshAsync();
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
            foreach (var row in _rows) row.IsSelected = false; UpdateSelectionSettings();
            Tab_Click(AccountsTab, new RoutedEventArgs());
            modal = ShowAccountAsync(_rows[1]); await Task.Delay(350);
            DialogRejoin.IsChecked = true;
            InvokeDialogButton("CloseButton"); await modal.WaitAsync(TimeSpan.FromSeconds(5));
            Assert(!_rows[1].Account.AutoRecovery, "Cancel discards individual rejoin changes.");
            modal = ShowAccountAsync(_rows[1]); await Task.Delay(350);
            DialogRejoin.IsChecked = true;
            await CaptureAsync(directory, "account-rejoin-settings.png", _accountDialog);
            InvokeDialogButton("PrimaryButton"); await modal.WaitAsync(TimeSpan.FromSeconds(5));
            state = await _api.GetAsync<Snapshot>("status");
            Assert(state.Accounts.Single(a => a.Id == "1").AutoRecovery && Selected().Length == 0,
                "Account settings save rejoin without requiring table selection.");
            Tab_Click(PresetsTab, new RoutedEventArgs());
            var resetLookup = ShowPresetLookupAsync(); await Task.Delay(350);
            PresetClear_Click(PresetAccounts, new RoutedEventArgs());
            InvokeDialogButton("PrimaryButton"); await resetLookup;
            Assert(FindVisual<ListView>(PresetsPage, "PresetAccounts") is null, "Preset page keeps its account table inside the lookup modal.");
            var lookup = ShowPresetLookupAsync(); await Task.Delay(350);
            Assert(_modalOpen && FindVisual<Grid>(OpenDialog(), "PresetTableHeader") is not null, "Choose accounts opens a lookup modal.");
            PresetAccounts.SelectedItems.Add(_rows[0]);
            PresetSearch.Text = "sample_1"; await Task.Delay(100);
            Assert(_presetVisibleRows.Count == 1 && PresetLookupIds.Count == 1 && PresetLookupSummary.Text.Contains("hidden") && PresetSelected().Length == 0,
                "Lookup selection survives filters and remains a draft until confirmed.");
            PresetSelectVisible_Click(PresetAccounts, new RoutedEventArgs());
            Assert(PresetLookupIds.Count == 2, "Select visible adds matches without losing hidden choices.");
            PresetStatus.SelectedIndex = 1; await Task.Delay(100);
            Assert(_presetVisibleRows.Count == 0 && PresetLookupEmpty.Visibility == Visibility.Visible, "Lookup status filter combines with the search and displays no results.");
            PresetClearFilters_Click(_presetLookupDialog, new RoutedEventArgs()); await Task.Delay(100);
            Assert(_presetVisibleRows.Count == _rows.Count, "Clear filters restores the complete lookup table.");
            PresetRejoin.SelectedIndex = 2; await Task.Delay(100);
            Assert(_presetVisibleRows.All(row => !row.Account.AutoRecovery), "Lookup rejoin filter selects only accounts with rejoin off.");
            PresetClearFilters_Click(_presetLookupDialog, new RoutedEventArgs()); await Task.Delay(100);
            Assert(PresetTableHeader.ColumnDefinitions.Count == 6 && FindVisual<Grid>(PresetAccounts, "PresetAccountRow") is { } lookupRow && lookupRow.ColumnDefinitions.Count == 6,
                "Modal lookup renders six account columns.");
            await CaptureAsync(directory, "preset-account-lookup.png", _presetLookupDialog);
            InvokeDialogButton("CloseButton"); await lookup;
            Assert(PresetSelected().Length == 0, "Cancel discards all draft account choices.");
            lookup = ShowPresetLookupAsync(); await Task.Delay(350);
            Assert(PresetAccounts.SelectedItems.Count == 0, "Reopening a cancelled lookup restores the saved selection.");
            PresetAccounts.SelectedItems.Add(_rows[0]);
            InvokeDialogButton("PrimaryButton"); await lookup;
            Assert(PresetSelected().Length == 1, "Use selected accounts commits lookup choices.");
            lookup = ShowPresetLookupAsync(); await Task.Delay(350);
            PresetClear_Click(PresetAccounts, new RoutedEventArgs());
            Assert(PresetLookupIds.Count == 0, "Clear selection clears the modal draft.");
            InvokeDialogButton("CloseButton"); await lookup;
            Assert(PresetSelected().Length == 1, "Cancel preserves previously confirmed choices.");
            ProfileName.Text = "Single account";
            Assert(SavePresetButton.IsEnabled, "Preset creation needs only local selection and a name.");
            await SaveProfileAsync(false);
            Assert(_profiles.Single(p => p.Name == "Single account").Entries.Length == 1, "Preset saves only locally chosen accounts.");
            ProfilePicker.SelectedItem = _profiles.Single(p => p.Name == "Single account");
            Root.UpdateLayout();
            Assert(Math.Abs(LaunchPresetButton.ActualHeight - ApplyPresetButton.ActualHeight) < 0.5 && Math.Abs(SavePresetButton.ActualHeight - PresetEditButton.ActualHeight) < 0.5, "Preset primary and secondary buttons have consistent heights.");
            await CaptureAsync(directory, "presets-redesigned.png");
            PageScroll.ChangeView(null, 10000, null, true); await Task.Delay(150);
            await CaptureAsync(directory, "presets-minimal.png");
            foreach (var theme in new[] { 1, 2 })
            {
                ThemePicker.SelectedIndex = theme;
                var reviewDialog = CreatePresetReview((LaunchProfile)ProfilePicker.SelectedItem, "start");
                var reviewTask = reviewDialog.ShowAsync(); await Task.Delay(350);
                var applyButton = FindVisual<Button>(reviewDialog, "PrimaryButton")!;
                var cancelButton = FindVisual<Button>(reviewDialog, "CloseButton")!;
                Assert(reviewDialog.PrimaryButtonStyle != reviewDialog.CloseButtonStyle && applyButton.Background is SolidColorBrush applyInk && cancelButton.Background is SolidColorBrush cancelInk && applyInk.Color != cancelInk.Color, "Apply and Cancel render with distinct backgrounds in both themes.");
                await CaptureAsync(directory, $"preset-review-{(theme == 1 ? "light" : "dark")}.png", reviewDialog);
                InvokeDialogButton("CloseButton"); await reviewTask;
            }

            Tab_Click(SettingsTab, new RoutedEventArgs());
            ThemePicker.SelectedIndex = 2; await Task.Delay(200);
            Assert(Root.ActualTheme == ElementTheme.Dark, "Dark theme applies immediately.");
            Assert(ThemePath is not null && File.ReadAllText(ThemePath) == "Dark", "Theme preference persists in the isolated data directory.");
            foreach (var tab in new[] { SettingsTab, AccountsTab })
            {
                Assert(tab.Focus(FocusState.Keyboard), "Sidebar tab accepts keyboard focus.");
                foreach (var visualState in tab.IsChecked == true
                    ? new[] { "Checked", "CheckedPointerOver", "CheckedPressed" }
                    : new[] { "Normal", "PointerOver", "Pressed" })
                {
                    Assert(VisualStateManager.GoToState(tab, visualState, false), $"Sidebar state {visualState} exists.");
                    Root.UpdateLayout();
                    var presenter = FindVisual<ContentPresenter>(tab, "ContentPresenter")!;
                    Assert(presenter.Foreground is SolidColorBrush ink && ink.Color.R > 200 && ink.Color.G > 200 && ink.Color.B > 200,
                        $"Dark sidebar {tab.Name} {visualState} stays light while keyboard focused.");
                    await CaptureAsync(directory, $"sidebar-dark-{tab.Name}-{visualState}.png");
                }
                VisualStateManager.GoToState(tab, tab.IsChecked == true ? "Checked" : "Normal", false);
            }
            ThemePicker.Focus(FocusState.Programmatic);
            var themedGame = EditGameAsync(null); await Task.Delay(300);
            Assert(_gameDialog!.RequestedTheme == ElementTheme.Dark && _gameDialog.PrimaryButtonStyle is not null, "Dialogs follow theme and emphasize Save.");
            await CaptureAsync(directory, "game-editor-dark.png", _gameDialog);
            InvokeDialogButton("CloseButton"); await themedGame;
            ThemePicker.SelectedIndex = 1; await Task.Delay(200);
            Assert(Root.ActualTheme == ElementTheme.Light, "Light theme applies immediately.");
            await CaptureAsync(directory, "appearance-light.png");
            ThemePicker.SelectedIndex = 0; await Task.Delay(200);
            Assert(Root.RequestedTheme == ElementTheme.Default, "System theme follows device preference.");
            Tab_Click(AccountsTab, new RoutedEventArgs());
            var hwnd = WindowNative.GetWindowHandle(this);
            var scale = NativeTray.Scale(hwnd);
            AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd)).Resize(new Windows.Graphics.SizeInt32((int)(1700 * scale), (int)(940 * scale)));
            await Task.Delay(350);
            Assert(Math.Abs(AccountTable.ActualWidth - Math.Max(AccountTable.MinWidth, AccountTableScroll.ActualWidth)) < 2,
                $"Account table fills the available viewport while retaining its scrollable minimum (table {AccountTable.ActualWidth}, viewport {AccountTableScroll.ActualWidth}).");
            await CaptureAsync(directory, "accounts-wide.png");
            AppWindow.GetFromWindowId(Win32Interop.GetWindowIdFromWindow(hwnd)).Resize(new Windows.Graphics.SizeInt32((int)(1020 * scale), (int)(780 * scale)));
            await Task.Delay(350);
            foreach (var tab in new[] { AccountsTab, GamesTab, RecoveryTab, PresetsTab, SettingsTab, DiscordTab, BackupsTab, UpdatesTab, SupportTab })
            {
                Tab_Click(tab, new RoutedEventArgs()); await Task.Delay(80); Root.UpdateLayout();
                var available = PageScroll.ActualWidth - PageScroll.Padding.Left - PageScroll.Padding.Right;
                Assert(PageContent.ActualWidth <= available + 1, $"Page {tab.Name} fits the minimum window width.");
                await CaptureAsync(directory, $"minimum-{tab.Name}.png");
                if (tab == PresetsTab)
                {
                    var minimumLookup = ShowPresetLookupAsync(); await Task.Delay(350);
                    Assert(((FrameworkElement)OpenDialog()).ActualWidth <= Root.ActualWidth && ((FrameworkElement)OpenDialog()).ActualHeight <= Root.ActualHeight,
                        "Account lookup modal fits the minimum window size.");
                    await CaptureAsync(directory, "preset-lookup-minimum.png", _presetLookupDialog);
                    InvokeDialogButton("CloseButton"); await minimumLookup;
                }
            }
            Tab_Click(AccountsTab, new RoutedEventArgs());
            await CaptureAsync(directory, "accounts-small.png");
            Tab_Click(RecoveryTab, new RoutedEventArgs()); await RefreshAsync();
            await CaptureAsync(directory, "dashboard-small.png");
            Assert(RecoveryPage.ActualWidth <= PageScroll.ActualWidth - PageScroll.Padding.Left - PageScroll.Padding.Right + 1 && StatusBreakdown.ActualWidth > 0, "Dashboard chart fits the smaller window.");
            Tab_Click(AccountsTab, new RoutedEventArgs());
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
            await CaptureAsync(directory, "account-modal-small.png", _accountDialog);
            await _api.SendAsync(System.Net.Http.HttpMethod.Delete, $"accounts/{rowToRemove.Id}");
            await RefreshAsync();
            Assert(!_accountDialog.IsPrimaryButtonEnabled && EditorError.IsOpen, "External removal disables stale modal save.");
            InvokeDialogButton("CloseButton");
            await modal.WaitAsync(TimeSpan.FromSeconds(5));
            foreach (var row in _rows) row.IsSelected = false; UpdateSelectionSettings();
            _rows.Clear();
            ApplyFilters();
            await CaptureAsync(directory, "empty.png");
            File.WriteAllText(Path.Combine(directory, "result.txt"), "WinUI smoke passed: animated page fade/slide, rapid navigation cancellation, sliding selection across all sidebar groups, reduced-motion path and current-page scroll retention; header select/clear, saved bot controls and status, game hover animation, sidebar scroll reset, saved game create/edit/account selection, close preference, changelog modal; bulk settings Save/Cancel, atomic invalid/rejected saves, alias patterns, selected/all account scopes, groups and filtering, saved profile review/apply, backup picker, recovery/history and account filtering; inherited toast, modal, deletion, selection, bulk-action, sign-in, resize and rendering checks.");
        }
        catch (Exception ex)
        {
            File.WriteAllText(Path.Combine(directory, "result.txt"), $"FAILED: {ex.GetType().Name}: {ex.Message}\n{ex.StackTrace}");
        }
        _exiting = true;
        Close();
    }

    private void AssertUpdateButtonLayout()
    {
        Root.UpdateLayout();
        var position = UpdateNoticeButton.TransformToVisual(UpdateNotice).TransformPoint(new Windows.Foundation.Point());
        var message = FindVisualText(UpdateNotice, UpdateNotice.Message);
        Assert(message is not null && message.ActualHeight > 0, "The update banner message is visible.");
        var messagePosition = message!.TransformToVisual(UpdateNotice).TransformPoint(new Windows.Foundation.Point());
        var stacked = position.Y >= messagePosition.Y + message.ActualHeight - 1;
        Assert(UpdateNoticeButton.ActualHeight > 0 && position.X >= 0 && position.Y >= 0 &&
            position.X + UpdateNoticeButton.ActualWidth <= UpdateNotice.ActualWidth + 1 &&
            position.Y + UpdateNoticeButton.ActualHeight <= UpdateNotice.ActualHeight + 1 &&
            (stacked || Math.Abs(position.Y + UpdateNoticeButton.ActualHeight / 2 - UpdateNotice.ActualHeight / 2) < 1),
            $"The update action fits below wrapped content or is centered beside it (top {position.Y}, button {UpdateNoticeButton.ActualHeight}, banner {UpdateNotice.ActualHeight}).");
    }
    private static TextBlock? FindVisualText(DependencyObject root, string text)
    {
        if (root is TextBlock block && block.Text == text) return block;
        for (var i = 0; i < VisualTreeHelper.GetChildrenCount(root); i++)
            if (FindVisualText(VisualTreeHelper.GetChild(root, i), text) is { } match) return match;
        return null;
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
        if (_pageTransition?.GetCurrentState() == Microsoft.UI.Xaml.Media.Animation.ClockState.Active ||
            _selectionTransition?.GetCurrentState() == Microsoft.UI.Xaml.Media.Animation.ClockState.Active) await Task.Delay(160);
        var bitmap = new RenderTargetBitmap();
        if (element is ContentDialog) element = OpenDialog();
        // RenderTargetBitmap omits the OS Mica backdrop. Preview its neutral fallback
        // so transparent XAML layers remain legible in standalone fixture PNGs.
        var background = Root.Background;
        try
        {
            if (element is null) Root.Background = new SolidColorBrush(Root.ActualTheme == ElementTheme.Dark
                ? Windows.UI.Color.FromArgb(255, 32, 32, 32) : Windows.UI.Color.FromArgb(255, 243, 243, 243));
            await bitmap.RenderAsync(element ?? Root);
        }
        finally { Root.Background = background; }
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
