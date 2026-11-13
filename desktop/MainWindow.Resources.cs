using System.Net.Http;
using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;

namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private CancellationTokenSource? _resourceStreamCts;
    private ResourceSnapshotResponse? _resourceSnapshot;
    private ResourceHistoryPoint[] _resourceHistory = [];
    private string _resourceSortBy = "cpu";
    private bool _resourceSortDesc = true;
    private bool _resourceLoading;

    private void StartResourcesMonitoring()
    {
        if (_disposed) return;
        if (_resourceStreamCts is not null) return;
        _resourceStreamCts = new CancellationTokenSource();
        var token = _resourceStreamCts.Token;
        _ = RunResourceMonitorLoopAsync(token);
    }

    private void StopResourcesMonitoring()
    {
        if (_resourceStreamCts is null) return;
        try { _resourceStreamCts.Cancel(); } catch (ObjectDisposedException) { }
        _resourceStreamCts.Dispose();
        _resourceStreamCts = null;
    }

    private async Task RunResourceMonitorLoopAsync(CancellationToken token)
    {
        _resourceLoading = _resourceSnapshot is null;
        RenderResourcesState();
        while (!token.IsCancellationRequested && !_disposed)
        {
            try
            {
                var initial = await _api.GetAsync<ResourceSnapshotResponse>("resources?history=true&seconds=60");
                if (token.IsCancellationRequested || _disposed) return;
                ApplyResourceSnapshot(initial);
                await foreach (var update in _api.StreamResourcesAsync(token))
                {
                    if (token.IsCancellationRequested || _disposed) return;
                    ApplyResourceSnapshot(update);
                }
            }
            catch (OperationCanceledException) { return; }
            catch (Exception ex) when (ex is ApiException or HttpRequestException or JsonException or ObjectDisposedException)
            {
                if (_disposed || token.IsCancellationRequested) return;
                _resourceLoading = false;
                ResourceBanner.Title = "Resource stream paused";
                ResourceBanner.Message = "Unable to stream live resource telemetry cleanly. Retrying automatically…";
                ResourceBanner.Severity = InfoBarSeverity.Warning;
                ResourceBanner.IsOpen = true;
                ResourceSamplerStatus.Text = "Disconnected · retrying…";
            }

            try { await Task.Delay(2000, token); }
            catch (OperationCanceledException) { return; }
        }
    }

    private void ApplyResourceSnapshot(ResourceSnapshotResponse snapshot)
    {
        _resourceLoading = false;
        _resourceSnapshot = snapshot;
        _resourceHistory = ResourceFormatting.MergeHistory(_resourceHistory, snapshot, snapshot.Metadata.HistoryCapacity > 0 ? snapshot.Metadata.HistoryCapacity : 60);
        RenderResourcesState();
    }

    private async void ResourceRefresh_Click(object sender, RoutedEventArgs e)
    {
        if (_disposed) return;
        ResourceRefreshButton.IsEnabled = false;
        try
        {
            await GuardAsync(async () =>
            {
                var snapshot = await _api.GetAsync<ResourceSnapshotResponse>("resources?history=true&seconds=60");
                ApplyResourceSnapshot(snapshot);
            });
        }
        finally { ResourceRefreshButton.IsEnabled = true; }
    }

    private void ResourceSort_Changed(object sender, SelectionChangedEventArgs e)
    {
        if (ResourceSortBy?.SelectedItem is ComboBoxItem item && item.Tag is string tag)
        {
            _resourceSortBy = tag;
            RenderResourcesState();
        }
    }

    private void ResourceSortDirection_Click(object sender, RoutedEventArgs e)
    {
        _resourceSortDesc = !_resourceSortDesc;
        ResourceSortDirectionButton.Content = _resourceSortDesc ? "High → Low" : "Low → High";
        RenderResourcesState();
    }

    private void RenderResourcesState()
    {
        if (ResourcesPage is null || SystemCpuMetric is null || ResourceEmptyState is null || CpuHistoryBars is null || ResourceInstanceList is null) return;
        var snap = _resourceSnapshot;
        if (snap is null)
        {
            ResourceSamplerStatus.Text = _resourceLoading ? "Connecting to sampler…" : "Waiting for first resource sample…";
            RunningInstancesMetric.Text = "—";
            RunningInstancesHint.Text = "Managed Roblox instances";
            SystemCpuMetric.Text = "—";
            SystemCpuHint.Text = "Logical processors: —";
            SystemRamMetric.Text = "—";
            SystemRamHint.Text = "Physical memory";
            ManagedUsageMetric.Text = "—";
            ManagedUsageHint.Text = "CPU & RAM aggregate";
            ResourceInstanceList.ItemsSource = Array.Empty<ResourceInstanceRow>();
            ResourceEmptyState.Visibility = Visibility.Visible;
            ResourceEmptyTitle.Text = _resourceLoading ? "Loading resource monitor…" : "No resource sample yet";
            ResourceEmptyHint.Text = "Waiting for verified metrics from the Rust engine.";
            ResourceBanner.IsOpen = false;
            return;
        }

        var localTime = snap.Timestamp.ToLocalTime().ToString("HH:mm:ss");
        var modeText = snap.Metadata.SamplingMode switch
        {
            "active" => "Active (1s)",
            "on_demand" => "On-demand",
            _ => "Suspended"
        };
        ResourceSamplerStatus.Text = $"Updated {localTime} · age {snap.SampleAgeMs} ms · {modeText} · CPU {snap.Metadata.CpuNormalization}";

        RunningInstancesMetric.Text = snap.Aggregates.RunningInstances.ToString();
        var unmanagedNote = snap.Aggregates.UnmanagedRobloxProcesses is > 0
            ? $" · {snap.Aggregates.UnmanagedRobloxProcesses} unmanaged"
            : "";
        var staleNote = snap.Aggregates.ExitedOrStaleInstances > 0
            ? $" · {snap.Aggregates.ExitedOrStaleInstances} attention"
            : "";
        RunningInstancesHint.Text = $"of {snap.Aggregates.TotalAccounts} saved accounts{unmanagedNote}{staleNote}";

        SystemCpuMetric.Text = ResourceFormatting.FormatPercentMetric(snap.System.CpuPercent, snap.System.CpuAvailability);
        var sysGpu = snap.System.GpuAvailability switch
        {
            "available" when snap.System.GpuPercent is { } g => $"GPU {g:F1}%",
            "pending" => "GPU Sampling…",
            _ => "GPU Unavailable"
        };
        SystemCpuHint.Text = $"{snap.System.LogicalProcessors} logical CPUs · {sysGpu}";

        SystemRamMetric.Text = ResourceFormatting.FormatPercentMetric(snap.System.RamUtilizationPercent, snap.System.RamAvailability);
        SystemRamHint.Text = snap.System.UsedRamBytes is { } used && snap.System.TotalRamBytes is { } total
            ? $"{ResourceFormatting.FormatBytes(used)} / {ResourceFormatting.FormatBytes(total)}"
            : "Physical memory unavailable";

        var managedCpu = snap.Aggregates.CpuAvailability == "available"
            ? $"{snap.Aggregates.CpuPercent:F1}%"
            : ResourceFormatting.FormatPercentMetric(null, snap.Aggregates.CpuAvailability);
        ManagedUsageMetric.Text = managedCpu;
        var managedPriv = snap.Aggregates.PrivateMemoryBytes is { } p
            ? $" · Priv {ResourceFormatting.FormatBytes(p)}"
            : "";
        var managedGpu = snap.Aggregates.GpuAvailability switch
        {
            "available" when snap.Aggregates.GpuPercent is { } mg => $" · GPU {mg:F1}%",
            _ => ""
        };
        ManagedUsageHint.Text = $"{ResourceFormatting.FormatBytes(snap.Aggregates.WorkingSetBytes)}{managedPriv}{managedGpu}";

        if (snap.Stale)
        {
            ResourceBanner.Title = "Stale resource snapshot";
            ResourceBanner.Message = $"Latest sample is {snap.SampleAgeMs / 1000.0:F1}s old. Waiting for fresh backend metrics.";
            ResourceBanner.Severity = InfoBarSeverity.Warning;
            ResourceBanner.IsOpen = true;
        }
        else if (snap.Availability == "unavailable")
        {
            ResourceBanner.Title = "Host telemetry unavailable";
            ResourceBanner.Message = "Windows process or host metrics could not be read cleanly.";
            ResourceBanner.Severity = InfoBarSeverity.Warning;
            ResourceBanner.IsOpen = true;
        }
        else if (snap.Availability == "pending")
        {
            ResourceBanner.Title = "Warming up baseline";
            ResourceBanner.Message = "Collecting initial CPU and GPU counter deltas…";
            ResourceBanner.Severity = InfoBarSeverity.Informational;
            ResourceBanner.IsOpen = true;
        }
        else if (snap.Aggregates.ExitedOrStaleInstances > 0)
        {
            ResourceBanner.Title = "Process state mismatch or exit detected";
            ResourceBanner.Message = $"{snap.Aggregates.ExitedOrStaleInstances} tracked instance(s) exited, denied access, or failed PID creation-time verification.";
            ResourceBanner.Severity = InfoBarSeverity.Warning;
            ResourceBanner.IsOpen = true;
        }
        else
        {
            ResourceBanner.IsOpen = false;
        }

        RenderHistoryBars(CpuHistoryBars, _resourceHistory.Select(h => (h.SystemCpuPercent ?? 0, h.ManagedCpuPercent)).ToArray(), 100.0);
        var hostRamBytes = (double)(snap.System.TotalRamBytes ?? 0);
        var managedRamScale = hostRamBytes > 0
            ? hostRamBytes
            : Math.Max(256.0 * 1024 * 1024, (double)_resourceHistory.Select(h => h.ManagedRamBytes).DefaultIfEmpty(0UL).Max());
        RenderHistoryBars(RamHistoryBars, _resourceHistory.Select(h => (h.RamUtilizationPercent ?? 0, h.ManagedRamBytes / managedRamScale * 100.0)).ToArray(), 100.0);
        var latestHist = _resourceHistory.LastOrDefault();
        CpuHistoryCaption.Text = latestHist is null
            ? "System (muted) · Managed Roblox (accent)"
            : $"Latest: System {latestHist.SystemCpuPercent?.ToString("F1") ?? "—"}% · Managed {latestHist.ManagedCpuPercent:F1}%";
        RamHistoryCaption.Text = latestHist is null
            ? "Host RAM % (muted) · Managed Working Set (accent)"
            : $"Latest: Host RAM {latestHist.RamUtilizationPercent?.ToString("F1") ?? "—"}% · Managed {ResourceFormatting.FormatBytes(latestHist.ManagedRamBytes)}";

        var sorted = ResourceFormatting.SortInstances(snap.Accounts, _resourceSortBy, _resourceSortDesc);
        var rows = sorted.Select(a => new ResourceInstanceRow(a)).ToArray();
        ResourceInstanceList.ItemsSource = rows;
        if (rows.Length == 0)
        {
            ResourceEmptyState.Visibility = Visibility.Visible;
            ResourceEmptyTitle.Text = "No saved accounts";
            ResourceEmptyHint.Text = "Add an account to monitor its managed Roblox instance.";
        }
        else if (snap.Aggregates.RunningInstances == 0)
        {
            ResourceEmptyState.Visibility = Visibility.Visible;
            ResourceEmptyTitle.Text = "No running managed Roblox instances";
            ResourceEmptyHint.Text = "Stopped and exited accounts appear below until started.";
        }
        else
        {
            ResourceEmptyState.Visibility = Visibility.Collapsed;
        }
    }

    private void RenderHistoryBars(Grid host, (double TotalPct, double ManagedPct)[] points, double maximum)
    {
        host.Children.Clear();
        host.ColumnDefinitions.Clear();
        if (points.Length == 0) return;

        var totalBrush = ThemeInk();
        var managedBrush = new SolidColorBrush(Root.ActualTheme == ElementTheme.Dark
            ? Windows.UI.Color.FromArgb(255, 175, 203, 255)
            : Windows.UI.Color.FromArgb(255, 37, 99, 235));
        const double maxHeight = 96.0;

        for (var i = 0; i < points.Length; i++)
        {
            host.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var cell = new Grid { Height = maxHeight, VerticalAlignment = VerticalAlignment.Bottom };
            var sysH = Math.Max(2.0, Math.Clamp(points[i].TotalPct / maximum, 0, 1) * maxHeight);
            var mgdH = Math.Max(points[i].ManagedPct > 0 ? 3.0 : 0.0, Math.Clamp(points[i].ManagedPct / maximum, 0, 1) * maxHeight);

            cell.Children.Add(new Border
            {
                Height = sysH,
                VerticalAlignment = VerticalAlignment.Bottom,
                Background = totalBrush,
                Opacity = 0.28,
                CornerRadius = new CornerRadius(2),
                Margin = new Thickness(1, 0, 1, 0)
            });
            if (mgdH > 0)
            {
                cell.Children.Add(new Border
                {
                    Height = mgdH,
                    VerticalAlignment = VerticalAlignment.Bottom,
                    Background = managedBrush,
                    Opacity = 0.9,
                    CornerRadius = new CornerRadius(2),
                    Margin = new Thickness(1, 0, 1, 0)
                });
            }
            Grid.SetColumn(cell, i);
            host.Children.Add(cell);
        }
    }
}
