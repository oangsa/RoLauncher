using System.Net.Http;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private readonly DispatcherTimer _updateTimer = new() { Interval = TimeSpan.FromHours(6) };
    private readonly CancellationTokenSource _updateLifetime = new();
    private CancellationTokenSource? _updateDownloadCancellation;
#if UI_SMOKE
    private readonly UpdateDownloader _updater = new(Path.Combine(Environment.GetEnvironmentVariable("ROLAUNCHER_UI_SMOKE_DIR")!, "update-cache"));
#else
    private readonly UpdateDownloader _updater = new(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "RoLauncher", "updates"));
#endif
    private DownloadedInstaller? _downloadedUpdate;
    private bool _checkingUpdate, _downloadingUpdate, _installingUpdate;

    private void StartUpdateChecks()
    {
        _updateTimer.Tick += async (_, _) => await CheckForUpdateAsync(false);
        _updateTimer.Start();
        _ = CheckForUpdateAsync(false);
    }

    private async void CheckUpdates_Click(object sender, RoutedEventArgs e) => await CheckForUpdateAsync(true);

    private async Task CheckForUpdateAsync(bool manual)
    {
        if (_checkingUpdate || _downloadingUpdate || _installingUpdate || _disposed) return;
        _checkingUpdate = true; CheckUpdatesButton.IsEnabled = false;
        try
        {
            var result = (await _api.SendAsync(HttpMethod.Post, "updates/official/check"))!.Value;
            var view = result.Deserialize<UpdateView>(ApiClient.JsonOptions) ?? throw new InvalidDataException("Invalid release information.");
            if (_disposed) return;
            if (view.Available)
            {
                UpdateDownloader.ValidateRelease(view, _config.Version);
                _update = view;
                _downloadedUpdate = null;
                _downloadedUpdate = await _updater.CachedAsync(view, _config.Version, _updateLifetime.Token);
                UpdateNotice.IsOpen = true;
                UpdateNotice.Visibility = Visibility.Visible;
                UpdateNotice.Title = $"RoLauncher {view.Version} is available";
                SetUpdateMessage(_downloadedUpdate is null ? "Download the update, then relaunch when you’re ready." : "The update is downloaded and verified. Relaunch when you’re ready.");
            }
            else
            {
                _update = view;
                _downloadedUpdate = null;
                UpdateNotice.IsOpen = false;
                UpdateNotice.Visibility = Visibility.Collapsed;
                UpdateStatus.Text = $"You’re up to date · v{_config.Version}";
                if (manual) FeedbackMessage("RoLauncher is up to date.", InfoBarSeverity.Success);
            }
            UpdateReleaseNotesButton.IsEnabled = true;
        }
        catch (OperationCanceledException) when (_updateLifetime.IsCancellationRequested) { }
        catch (Exception ex) when (ex is ApiException or HttpRequestException or InvalidDataException or IOException or UnauthorizedAccessException or JsonException or OperationCanceledException or ObjectDisposedException)
        {
            if (!_disposed)
            {
                UpdateStatus.Text = "Could not check for updates. RoLauncher will try again automatically.";
                if (manual) FeedbackMessage(UpdateStatus.Text, InfoBarSeverity.Warning);
            }
        }
        finally
        {
            _checkingUpdate = false;
            if (!_disposed) { CheckUpdatesButton.IsEnabled = true; UpdateActions(); }
        }
    }

    private void SetUpdateMessage(string message) { UpdateStatus.Text = message; UpdateNotice.Message = message; }
    private void UpdateActions()
    {
        var text = _installingUpdate ? "Installing…" : _downloadingUpdate ? "Cancel download" : _downloadedUpdate is not null ? "Relaunch" : "Download";
        UpdateDownloadButton.Content = UpdateNoticeButton.Content = text;
        UpdateDownloadButton.IsEnabled = UpdateNoticeButton.IsEnabled = !_checkingUpdate && !_installingUpdate && (_update?.Available == true);
    }

    private async void UpdateAction_Click(object sender, RoutedEventArgs e)
    {
        if (_downloadingUpdate) { _updateDownloadCancellation?.Cancel(); return; }
        if (_installingUpdate || _update?.Available != true) return;
        if (_downloadedUpdate is not null) { await ApplyDownloadedUpdateAsync(); return; }
        _downloadingUpdate = true;
        _updateDownloadCancellation = CancellationTokenSource.CreateLinkedTokenSource(_updateLifetime.Token);
        CheckUpdatesButton.IsEnabled = false; UpdateActions();
        try
        {
            var progress = new Progress<int>(percentage =>
            {
                if (!_disposed && _downloadingUpdate && _downloadedUpdate is null) SetUpdateMessage(percentage < 0 ? "Downloading the update…" : $"Downloading the update… {percentage}%");
            });
            _downloadedUpdate = await _updater.DownloadAsync(_update, _config.Version, progress, _updateDownloadCancellation.Token);
            if (!_disposed) SetUpdateMessage("The update is downloaded and verified. Relaunch when you’re ready; Roblox clients stay open.");
        }
        catch (OperationCanceledException) { if (!_disposed) SetUpdateMessage("Download cancelled. Your current version is still running."); }
        catch (Exception ex) when (ex is HttpRequestException or InvalidDataException or IOException or UnauthorizedAccessException or ObjectDisposedException)
        {
            if (!_disposed) { SetUpdateMessage("Could not download or verify the update. Try Download again."); FeedbackMessage(UpdateStatus.Text, InfoBarSeverity.Error); }
        }
        finally
        {
            _downloadingUpdate = false; _updateDownloadCancellation.Dispose(); _updateDownloadCancellation = null;
            if (!_disposed) { CheckUpdatesButton.IsEnabled = true; UpdateActions(); }
        }
    }

    private async Task ApplyDownloadedUpdateAsync()
    {
        if (_modalOpen || _saving) { FeedbackMessage("Finish editing before applying the update."); return; }
        _installingUpdate = true; CheckUpdatesButton.IsEnabled = false; UpdateActions();
        try
        {
            SetUpdateMessage("Creating a backup and preparing the installer…");
            await _api.SendAsync(HttpMethod.Post, "backups");
            await UpdateHandoff.PrepareAsync(_downloadedUpdate!, _config, _updateLifetime.Token);
            _exiting = true;
            Close();
        }
        catch (Exception ex) when (ex is ApiException or InvalidDataException or IOException or UnauthorizedAccessException or InvalidOperationException or System.ComponentModel.Win32Exception or HttpRequestException or OperationCanceledException)
        {
            if (!_disposed) { SetUpdateMessage("Could not prepare the update. RoLauncher is still running; try Relaunch again."); FeedbackMessage(UpdateStatus.Text, InfoBarSeverity.Error); }
        }
        finally
        {
            _installingUpdate = false;
            if (!_disposed) { CheckUpdatesButton.IsEnabled = true; UpdateActions(); }
        }
    }

    private async void OpenUpdate_Click(object sender, RoutedEventArgs e)
    {
        if (_update is null) return;
        await GuardAsync(async () => { if (!await Windows.System.Launcher.LaunchUriAsync(new Uri(_update.ReleaseUrl))) FeedbackMessage("Could not open the release notes.", InfoBarSeverity.Error); });
    }

    private void DisposeUpdateResources()
    {
        _updateTimer.Stop(); _updateLifetime.Cancel(); _updater.Dispose();
    }
}
