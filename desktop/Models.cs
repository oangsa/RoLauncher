using System.ComponentModel;

namespace RoLauncher.Desktop;

public sealed record Target(ulong PlaceId, string? JobId, string? PrivateServerLink);
public sealed record ProcessInfo(uint Pid);
public sealed record Account(string Id, string Username, string Alias, Target? Target,
    bool AutoRecovery, string Status, ProcessInfo? Process, uint Failures, string? LastError,
    string Group = "", string FallbackPolicy = "allow_public", DateTimeOffset? NextRetry = null,
    string RecoveryReason = "", bool PublicFallbackActive = false, bool DesiredRunning = false,
    DateTimeOffset? RunningSince = null, ulong LongestStreakSeconds = 0);
public sealed record ProfileEntry(string AccountId, string Alias, Target? Target, bool AutoRecovery, string FallbackPolicy, string Group);
public sealed record LaunchProfile(string Id, string Name, ProfileEntry[] Entries)
{
    public override string ToString() => $"{Name} · {Entries.Length} accounts";
}
public sealed record GameProfile(string Id, string Name, string GameName, string? ThumbnailUrl, Target Target)
{
    public string Destination => $"Place {Target.PlaceId} · {(Target.PrivateServerLink is not null ? "Private server" : Target.JobId is not null ? "Specific server" : "Public server")}";
    public override string ToString() => $"{Name} · {GameName}";
}
public sealed record Snapshot(Account[] Accounts, bool NetworkSuspended, string Compatibility, LaunchProfile[]? Profiles = null, string UpdateRepository = "", bool IncludeBetaUpdates = false, GameProfile[]? GameProfiles = null, bool CloseToTray = true);
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
    public string Retry => Account.NextRetry is { } time ? $"Next retry in {Math.Max(0, (int)(time - DateTimeOffset.UtcNow).TotalSeconds)}s · {time.ToLocalTime():T}" : Account.Status == "reconnecting" ? "Waiting through the 30-second reconnect grace" : "No retry scheduled";
    public string Recovery => Account.RecoveryReason switch { "Auth" => "Session expired · sign in again", "Permission" => "Experience denied access", "TargetUnavailable" => "Configured destination unavailable", "Unsupported" => "Integration compatibility needs attention", "RateLimit" => "Rate limited · waiting before retry", "Network" => "Connectivity interrupted", _ => Error };
    public bool CanRetry => Account.Status == "backoff" && Account.NextRetry is not null && Account.AutoRecovery && Account.DesiredRunning && Account.RecoveryReason != "RateLimit";
    public string Fallback => Account.PublicFallbackActive ? "Public fallback active" : Account.FallbackPolicy switch { "stay" => "Stay on destination", "pause" => "Pause and notify if unavailable", _ => "Public fallback allowed" };
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
        "stopped" => "Stopped", "queued" => "Queued", "launching" => "Launching",
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
