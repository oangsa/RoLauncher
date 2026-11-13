using System.ComponentModel;

namespace RoLauncher.Desktop;

public sealed record Target(ulong PlaceId, string? JobId, string? PrivateServerLink);
public sealed record ProcessInfo(uint Pid);
public sealed record Account(string Id, string Username, string Alias, Target? Target,
    bool AutoRecovery, string Status, ProcessInfo? Process, uint Failures, string? LastError,
    string Group = "", string FallbackPolicy = "allow_public", DateTimeOffset? NextRetry = null,
    string RecoveryReason = "", bool PublicFallbackActive = false, bool DesiredRunning = false,
    DateTimeOffset? RunningSince = null, ulong LongestStreakSeconds = 0, string? GameProfileId = null);
public sealed record ProfileEntry(string AccountId, string Alias, Target? Target, bool AutoRecovery, string FallbackPolicy, string Group, string? GameProfileId = null);
public sealed record LaunchProfile(string Id, string Name, ProfileEntry[] Entries)
{
    public override string ToString() => $"{Name} · {Entries.Length} accounts";
}
public sealed record GameProfile(string Id, string Name, string GameName, string? ThumbnailUrl, Target Target)
{
    public string Destination => $"Place {Target.PlaceId} · {(Target.PrivateServerLink is not null ? "Private server" : Target.JobId is not null ? "Specific server" : "Public server")}";
    public override string ToString() => $"{Name} · {GameName}";
}
public sealed record Snapshot(Account[] Accounts, bool NetworkSuspended, string Compatibility, LaunchProfile[]? Profiles = null, string UpdateRepository = "", bool IncludeBetaUpdates = false, GameProfile[]? GameProfiles = null, bool CloseToTray = true, string ServerSelection = "smart", string JoinType = "random", bool MultiInstanceEnabled = true, bool ActiveMultiInstance = true, bool ExternalBackgroundCleanupEnabled = false);
public sealed record UpdateView(bool Available, string Version, string ReleaseUrl, string DownloadUrl, string ChecksumsUrl, string InstallerUrl = "", bool Prerelease = false);
public sealed record Activity(DateTimeOffset Timestamp, string AccountId, ulong? PlaceId, string Status, uint Failures, string Message)
{
    public string? AccountLabel { get; init; }
    public string Summary => $"{Timestamp.ToLocalTime():g} · {AccountLabel ?? (AccountId.Length == 0 ? "RoLauncher" : $"Account {AccountId}")} · {Status.Replace('_', ' ')} · {Message}";
}
public sealed record Notice(DateTimeOffset Timestamp, string Account, string Title, string Message, ulong? PlaceId);
public sealed record BotView(bool Enabled, bool Configured, string GuildId, string[] AllowedUsers, string Status, string ConnectionState = "offline");
public sealed record DiscordView(bool Enabled, bool Configured, bool NotifyRecovery, string DeliveryStatus, Notice[] Recent, BotView? Bot = null);

public sealed class AccountRow(Account account) : INotifyPropertyChanged
{
    public Account Account { get; private set; } = account;
    private bool _isSelected;
    public bool IsSelected { get => _isSelected; set { if (_isSelected == value) return; _isSelected = value; PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(IsSelected))); } }
    public string Id => Account.Id;
    public string Alias => Account.Alias;
    public string Group => string.IsNullOrEmpty(Account.Group) ? "Ungrouped" : Account.Group;
    public string Retry => Account.NextRetry is { } time ? $"Retry in {Math.Max(0, (int)(time - DateTimeOffset.UtcNow).TotalSeconds)}s · {time.ToLocalTime():T}" : Account.Status == "reconnecting" ? "Waiting up to 30 seconds to reconnect" : "No retry scheduled";
    public string Recovery => Account.RecoveryReason switch { "Auth" => "Session expired · sign in again", "Permission" => "Experience denied access", "TargetUnavailable" => "Destination unavailable", "Unsupported" => "Integration needs attention", "RateLimit" => "Rate limited · retry pending", "Network" => "Connection lost", _ => Error };
    public bool CanRetry => Account.Status == "backoff" && Account.NextRetry is not null && Account.AutoRecovery && Account.DesiredRunning && Account.RecoveryReason != "RateLimit";
    public string Fallback => Account.PublicFallbackActive ? "Public fallback active" : Account.FallbackPolicy switch { "stay" => "Stay on destination", "pause" => "Pause and notify", _ => "Public fallback allowed" };
    public string Username => $"@{Account.Username}";
    public ulong RunningSeconds => Account.RunningSince is { } since ? (ulong)Math.Max(0, (DateTimeOffset.UtcNow - since).TotalSeconds) : 0;
    public ulong LongestSeconds => Math.Max(Account.LongestStreakSeconds, RunningSeconds);
    public string RunningTime => Account.RunningSince is null ? "—" : FormatDuration(RunningSeconds);
    public string LongestOnline => FormatDuration(LongestSeconds);
    public static string FormatDuration(ulong seconds) => seconds >= 86400
        ? $"{seconds / 86400}d {seconds % 86400 / 3600}h {seconds % 3600 / 60}m"
        : seconds >= 3600 ? $"{seconds / 3600}h {seconds % 3600 / 60}m {seconds % 60}s"
        : seconds >= 60 ? $"{seconds / 60}m {seconds % 60}s" : $"{seconds}s";
    public string Pid => Account.Process?.Pid.ToString() ?? "—";
    public string Status => Account.Status switch
    {
        "needs_attention" => "Needs attention",
        "stopped" => "Stopped", "stopping" => "Stopping", "queued" => "Queued", "launching" => "Launching",
        "running" => "Running", "reconnecting" => "Reconnecting", "backoff" => "Backoff", _ => "Unknown"
    };
    private string? _gameName;
    public string Destination => Account.Target is null ? "Choose a game" : _gameName ?? $"Place {Account.Target.PlaceId}";
    public void SetGameName(string? name)
    {
        if (_gameName == name) return;
        _gameName = name;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Destination)));
    }
    public string DestinationKind => Account.Target?.PrivateServerLink is not null ? "Private server" : Account.Target?.JobId is not null ? "Specific server" : "Public server";
    public string Failures => Account.Failures.ToString();
    public string RecoveryAttempts => $"{Account.Failures}/5 consecutive failed attempts";
    public string Rejoin => Account.AutoRecovery ? "On" : "Off";
    public string Error => (Account.LastError ?? "—").Replace("recovery", "rejoin").Replace("Recovery", "Rejoin");
    public void Update(Account account)
    {
        if (Account.Target?.PlaceId != account.Target?.PlaceId) _gameName = null;
        Account = account;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(null));
    }
    public event PropertyChangedEventHandler? PropertyChanged;
}

public sealed record ResourceMetadata(
    string CpuNormalization = "machine_percent",
    string CpuFormula = "",
    string MemoryUnits = "bytes",
    string GpuNormalization = "",
    ulong SampleIntervalMs = 1000,
    int HistoryCapacity = 60,
    ulong StaleThresholdMs = 5000,
    string SamplingMode = "suspended",
    ulong LastSampleDurationMs = 0);

public sealed record SystemResources(
    uint LogicalProcessors,
    double? CpuPercent,
    string CpuAvailability,
    ulong? TotalRamBytes,
    ulong? AvailableRamBytes,
    ulong? UsedRamBytes,
    double? RamUtilizationPercent,
    string RamAvailability,
    double? GpuPercent,
    string GpuAvailability);

public sealed record ManagedAggregates(
    int RunningInstances,
    int TotalAccounts,
    double CpuPercent,
    string CpuAvailability,
    ulong WorkingSetBytes,
    ulong? PrivateMemoryBytes,
    double? GpuPercent,
    string GpuAvailability,
    int? UnmanagedRobloxProcesses,
    int? InaccessibleRobloxProcesses,
    int ExitedOrStaleInstances);

public sealed record ManagedInstanceResource(
    string AccountId,
    string DisplayName,
    string Username,
    string AccountStatus,
    uint? Pid,
    string? CreationTime,
    double? CpuPercent,
    ulong? WorkingSetBytes,
    ulong? PrivateMemoryBytes,
    double? GpuPercent,
    string GpuAvailability,
    ulong? UptimeSeconds,
    DateTimeOffset SampledAt,
    string Availability);

public sealed record ResourceHistoryPoint(
    DateTimeOffset Timestamp,
    double? SystemCpuPercent,
    double? RamUtilizationPercent,
    double ManagedCpuPercent,
    ulong ManagedRamBytes,
    double? ManagedGpuPercent,
    int RunningInstances);

public sealed record ResourceSnapshotResponse(
    DateTimeOffset Timestamp,
    ulong SampleAgeMs,
    bool Stale,
    string Availability,
    SystemResources System,
    ManagedAggregates Aggregates,
    ManagedInstanceResource[] Accounts,
    ResourceMetadata Metadata,
    ResourceHistoryPoint[]? History = null);

public sealed class ResourceInstanceRow(ManagedInstanceResource item)
{
    public ManagedInstanceResource Item { get; } = item;
    public string AccountId => Item.AccountId;
    public string DisplayName => string.IsNullOrWhiteSpace(Item.DisplayName) ? Item.Username : Item.DisplayName;
    public string Username => $"@{Item.Username}";
    public string Status => $"{ResourceFormatting.FormatAccountStatus(Item.AccountStatus)} · {ResourceFormatting.FormatProcessAvailability(Item.Availability)}";
    public string Pid => Item.Pid is { } pid ? pid.ToString() : "—";
    public string Cpu => Item.Availability switch
    {
        "available" when Item.CpuPercent is { } c => $"{c:F1}%",
        "pending" => "Sampling…",
        _ => "—"
    };
    public string Ram => Item.WorkingSetBytes is { } b ? ResourceFormatting.FormatBytes(b) : "—";
    public string PrivateRam => Item.PrivateMemoryBytes is { } b ? ResourceFormatting.FormatBytes(b) : "—";
    public string Gpu => Item.GpuAvailability switch
    {
        "available" when Item.GpuPercent is { } g => $"{g:F1}%",
        "pending" => "Sampling…",
        _ => "Unavailable"
    };
    public string Uptime => Item.UptimeSeconds is { } u ? AccountRow.FormatDuration(u) : "—";
}

public static class ResourceFormatting
{
    public static string FormatBytes(ulong bytes)
    {
        if (bytes < 1024) return $"{bytes} B";
        if (bytes < 1024 * 1024) return $"{bytes / 1024.0:F1} KB";
        if (bytes < 1024UL * 1024 * 1024) return $"{bytes / (1024.0 * 1024.0):F1} MB";
        return $"{bytes / (1024.0 * 1024.0 * 1024.0):F2} GB";
    }

    public static string FormatPercentMetric(double? value, string availability, string unavailableLabel = "Unavailable") =>
        availability switch
        {
            "available" when value is { } pct => $"{pct:F1}%",
            "pending" => "Sampling…",
            _ => unavailableLabel
        };

    public static string FormatAccountStatus(string status) => status switch
    {
        "needs_attention" => "Needs attention",
        "stopped" => "Stopped",
        "stopping" => "Stopping",
        "queued" => "Queued",
        "launching" => "Launching",
        "running" => "Running",
        "reconnecting" => "Reconnecting",
        "backoff" => "Backoff",
        _ => "Unknown"
    };

    public static string FormatProcessAvailability(string availability) => availability switch
    {
        "available" => "Live",
        "pending" => "Sampling",
        "exited" => "Exited",
        "inaccessible" => "Inaccessible",
        "ownership_mismatch" => "Identity mismatch",
        _ => "Unavailable"
    };

    public static ManagedInstanceResource[] SortInstances(
        IEnumerable<ManagedInstanceResource> source,
        string sortBy,
        bool descending)
    {
        static int CompareNullable<T>(T? a, T? b, bool desc) where T : struct, IComparable<T>
        {
            if (a is null && b is null) return 0;
            if (a is null) return 1;
            if (b is null) return -1;
            return desc ? b.Value.CompareTo(a.Value) : a.Value.CompareTo(b.Value);
        }

        var list = source.ToArray();
        Array.Sort(list, (left, right) =>
        {
            int primary = sortBy switch
            {
                "cpu" => CompareNullable(left.CpuPercent, right.CpuPercent, descending),
                "ram" => CompareNullable(left.WorkingSetBytes, right.WorkingSetBytes, descending),
                "uptime" => CompareNullable(left.UptimeSeconds, right.UptimeSeconds, descending),
                "pid" => CompareNullable(left.Pid, right.Pid, descending),
                "status" => descending
                    ? string.Compare(FormatAccountStatus(right.AccountStatus), FormatAccountStatus(left.AccountStatus), StringComparison.OrdinalIgnoreCase)
                    : string.Compare(FormatAccountStatus(left.AccountStatus), FormatAccountStatus(right.AccountStatus), StringComparison.OrdinalIgnoreCase),
                _ => descending
                    ? string.Compare(right.DisplayName, left.DisplayName, StringComparison.OrdinalIgnoreCase)
                    : string.Compare(left.DisplayName, right.DisplayName, StringComparison.OrdinalIgnoreCase)
            };
            if (primary != 0) return primary;
            var byName = string.Compare(left.DisplayName, right.DisplayName, StringComparison.OrdinalIgnoreCase);
            return byName != 0 ? byName : string.CompareOrdinal(left.AccountId, right.AccountId);
        });
        return list;
    }

    public static ResourceHistoryPoint[] MergeHistory(
        IReadOnlyList<ResourceHistoryPoint> current,
        ResourceSnapshotResponse snapshot,
        int capacity = 60)
    {
        var points = snapshot.History is { Length: > 0 } seeded
            ? new List<ResourceHistoryPoint>(seeded)
            : new List<ResourceHistoryPoint>(current);
        var next = new ResourceHistoryPoint(
            snapshot.Timestamp,
            snapshot.System.CpuPercent,
            snapshot.System.RamUtilizationPercent,
            snapshot.Aggregates.CpuPercent,
            snapshot.Aggregates.WorkingSetBytes,
            snapshot.Aggregates.GpuPercent,
            snapshot.Aggregates.RunningInstances);
        if (snapshot.History is null || snapshot.History.Length == 0)
        {
            if (points.Count == 0 || points[^1].Timestamp != next.Timestamp)
                points.Add(next);
        }
        while (points.Count > capacity) points.RemoveAt(0);
        return points.ToArray();
    }
}
