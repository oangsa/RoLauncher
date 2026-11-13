using RoLauncher.Desktop;
using System.Text.Json;

if (!CookieBatch.Parse(" first\r\n\r\nsecond\nfirst\rthird ").SequenceEqual(new[] { "first", "second", "third" }) || CookieBatch.Parse(" \r\n").Length != 0)
    throw new Exception("Cookie batches must trim lines, ignore blanks, and deduplicate without changing cookie contents.");

int passed = 0;
void Check(bool condition, string name)
{
    if (!condition) throw new Exception(name);
    passed++;
}
var account = new Account("123", "sample", "My account", new Target(1818, null, null), true, "running", new ProcessInfo(1234), 0, null);
var draft = EditorDraft.From(account);
Check(draft.TryTarget(out var target, out _) && target!.PlaceId == 1818, "Existing targets round-trip.");
draft.Place = "unfinished";
Check(!draft.TryTarget(out _, out _), "An incomplete draft cannot replace a saved destination.");
draft.Place = "0";
Check(!draft.TryTarget(out _, out _), "Place zero is invalid.");
draft.Place = "1818";
draft.Job = "not-a-uuid";
Check(!draft.TryTarget(out _, out _), "Invalid job IDs stay unsaved.");
draft.Job = Guid.NewGuid().ToString();
Check(draft.TryTarget(out target, out _) && target!.JobId == draft.Job, "Specific server target.");
draft.Private = "https://www.roblox.com/games/1818?privateServerLinkCode=ABC";
Check(!draft.TryTarget(out _, out _), "Conflicting destinations are rejected.");
draft.Job = "";
Check(draft.TryTarget(out target, out _) && target!.PrivateServerLink == draft.Private, "Private server target.");
draft.Private = "https://www.roblox.com/games/999?privateServerLinkCode=ABC";
Check(!draft.TryTarget(out _, out _), "Private link must match place.");
draft.Private = "https://evil.example/games/1818?privateServerLinkCode=ABC";
Check(!draft.TryTarget(out _, out _), "External links are rejected.");
draft.Private = "https://www.roblox.com/share?code=ABC&type=Server";
Check(draft.TryTarget(out _, out _), "Share server links are supported.");
draft.Private = "https://www.roblox.com/share?code=ABC&type=Experience";
Check(!draft.TryTarget(out _, out _), "Non-server shares are rejected.");
var row = new AccountRow(account);
Check(new AccountRow(account with { Status = "stopping" }).Status == "Stopping", "Pending shutdown is distinct from Stopped.");
Check(AccountFilter.Matches(account with { Status = "stopping" }, "", "stopping", ""), "Pending shutdown can be filtered.");
Check(row.Destination == "Place 1818", "Account game label falls back to the place number.");
row.SetGameName("Sample experience");
Check(row.Destination == "Sample experience", "Account game label uses resolved metadata.");
row.Update(account with { Status = "stopped" });
Check(row.Destination == "Sample experience", "Status polling retains resolved game metadata.");
row.Update(account with { Target = new Target(999, null, null) });
Check(row.Destination == "Place 999", "Changing a destination discards the previous game's label.");
row.Update(account with { Target = null });
Check(row.Destination == "Choose a game", "An account without a destination prompts game selection.");
row.Update(account);

Check(AccountFilter.Matches(account, " MY ACCOUNT ", "", ""), "Search ignores case and surrounding whitespace.");
Check(AccountFilter.Matches(account, "@SAMP", "running", "on"), "Search matches username with status and rejoin filters.");
Check(!AccountFilter.Matches(account, "missing", "", ""), "Unmatched names are excluded.");
Check(!AccountFilter.Matches(account, "sample", "stopped", ""), "Status combines with search.");
Check(!AccountFilter.Matches(account, "", "running", "off"), "Rejoin combines with status.");
Check(AccountFilter.Matches(account with { Status = "unknown" }, "", "unknown", ""), "Unknown status can be filtered.");
bool updated = false;
row.PropertyChanged += (_, _) => updated = true;
row.Update(account with { Status = "needs_attention", AutoRecovery = false, Alias = "Updated" });
Check(updated && row.Status == "Needs attention" && row.Rejoin == "Off" && row.Alias == "Updated", "Rows update in place, preserving selection identity.");
var streakRow = new AccountRow(account with { RunningSince = DateTimeOffset.UtcNow.AddHours(-2), LongestStreakSeconds = 9000, Status = "reconnecting" });
Check(streakRow.RunningSeconds >= 7200 && streakRow.LongestSeconds == 9000, "Reconnect time counts without replacing a longer saved streak.");
Check(streakRow.RunningTime.StartsWith("2h") && streakRow.LongestOnline == "2h 30m 0s", "Rows show running time and longest online.");
streakRow.Update(account with { LongestStreakSeconds = 9000 });
Check(streakRow.RunningTime == "—" && streakRow.LongestOnline == "2h 30m 0s", "Stopped accounts retain their best streak.");
Check(AccountRow.FormatDuration(90061) == "1d 1h 1m", "Long streaks include days.");
var options = new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower, PropertyNameCaseInsensitive = true };
var snapshot = JsonSerializer.Deserialize<Snapshot>("""{"accounts":[{"id":"1","username":"name","alias":"alias","target":{"place_id":1818,"job_id":null,"private_server_link":null},"auto_recovery":true,"status":"running","process":{"pid":999},"failures":2,"last_error":null}],"network_suspended":false,"compatibility":"test"}""", options)!;
Check(snapshot.Accounts[0].Process!.Pid == 999 && snapshot.Accounts[0].Target!.PlaceId == 1818, "Rust account schema deserializes correctly.");
var discord = JsonSerializer.Deserialize<DiscordView>("""{"enabled":false,"configured":true,"notify_recovery":true,"delivery_status":"saved","recent":[{"timestamp":"2026-10-05T00:00:00Z","account":"name","title":"Test","message":"message","place_id":1818}]}""", options)!;
Check(discord.Recent.Length == 1 && discord.Recent[0].Title == "Test", "Discord history uses Rust's recent schema.");
var bulk = new BulkDraft { ApplyGroup = true, Group = "Team", Fields = new() { Place = "invalid", Alias = "" } };
Check(bulk.TryPatch(out var patch, out _) && patch.Count == 1 && (string)patch["group"]! == "Team", "Unchecked mixed fields remain unchanged and are not validated.");
bulk.ApplyTarget = true;
Check(!bulk.TryPatch(out _, out _), "Bulk invalid destination rejects the whole draft before saving.");
bulk.ClearTarget = true;
Check(bulk.TryPatch(out patch, out _) && patch.ContainsKey("clear_target") && !patch.ContainsKey("target"), "Bulk destination clear is explicit.");
bulk.ApplyTarget = false; bulk.Group = "";
Check(bulk.TryPatch(out patch, out _) && (string)patch["group"]! == "", "Blank applied group removes membership.");
bulk.ApplyAlias = true; bulk.Fields.Alias = "Team {index} ({username})";
Check(bulk.TryPatch(out patch, out _) && (string)patch["alias"]! == bulk.Fields.Alias, "Alias patterns reach the backend for per-account expansion.");
bulk.ApplyRejoin = true; bulk.Rejoin = false; bulk.ApplyFallback = true; bulk.Fallback = "pause";
Check(bulk.TryPatch(out patch, out _) && (bool)patch["auto_recovery"]! == false && (string)patch["fallback_policy"]! == "pause", "Explicit false and pause policy are preserved.");
Check(!new BulkDraft().TryPatch(out _, out _), "Empty bulk edits require an Apply setting.");
var gameId = Guid.NewGuid().ToString();
var gameBulk = new BulkDraft { ApplyTarget = true, GameProfileId = gameId, Fields = new() { Place = "1818" } };
Check(gameBulk.TryPatch(out var gamePatch, out _) && (string)gamePatch["game_profile_id"]! == gameId, "Bulk saved-game assignments preserve preset identity.");
gameBulk.ClearTarget = true;
Check(gameBulk.TryPatch(out gamePatch, out _) && !gamePatch.ContainsKey("game_profile_id"), "Clearing a destination also clears its saved-game assignment.");
Check(snapshot.ServerSelection == "smart" && snapshot.JoinType == "random" && snapshot.Accounts[0].GameProfileId is null, "Older snapshots use compatible game-joining defaults.");
var retrying = new AccountRow(account with { Status = "backoff", NextRetry = DateTimeOffset.UtcNow.AddMinutes(1), DesiredRunning = true });
Check(retrying.CanRetry && retrying.Retry.Contains("Retry in"), "Scheduled retries expose a countdown and Retry now.");
retrying.Update(retrying.Account with { RecoveryReason = "RateLimit" });
Check(!retrying.CanRetry, "Rate-limit cooldown disables Retry now.");

var resourceJson = """
{
  "timestamp": "2026-11-13T00:00:00Z",
  "sample_age_ms": 250,
  "stale": false,
  "availability": "partial",
  "system": {
    "logical_processors": 8,
    "cpu_percent": 31.25,
    "cpu_availability": "available",
    "total_ram_bytes": 17179869184,
    "available_ram_bytes": 8589934592,
    "used_ram_bytes": 8589934592,
    "ram_utilization_percent": 50.0,
    "ram_availability": "available",
    "gpu_percent": null,
    "gpu_availability": "unavailable"
  },
  "aggregates": {
    "running_instances": 1,
    "total_accounts": 3,
    "cpu_percent": 12.5,
    "cpu_availability": "available",
    "working_set_bytes": 536870912,
    "private_memory_bytes": 419430400,
    "gpu_percent": null,
    "gpu_availability": "unavailable",
    "unmanaged_roblox_processes": 1,
    "inaccessible_roblox_processes": 0,
    "exited_or_stale_instances": 1
  },
  "accounts": [
    {
      "account_id": "1",
      "display_name": "Alpha",
      "username": "alpha",
      "account_status": "running",
      "pid": 1001,
      "creation_time": "100",
      "cpu_percent": 12.5,
      "working_set_bytes": 536870912,
      "private_memory_bytes": 419430400,
      "gpu_percent": null,
      "gpu_availability": "unavailable",
      "uptime_seconds": 3665,
      "sampled_at": "2026-11-13T00:00:00Z",
      "availability": "available"
    },
    {
      "account_id": "2",
      "display_name": "Beta",
      "username": "beta",
      "account_status": "stopped",
      "pid": null,
      "creation_time": null,
      "cpu_percent": null,
      "working_set_bytes": null,
      "private_memory_bytes": null,
      "gpu_percent": null,
      "gpu_availability": "unavailable",
      "uptime_seconds": null,
      "sampled_at": "2026-11-13T00:00:00Z",
      "availability": "exited"
    },
    {
      "account_id": "3",
      "display_name": "Gamma",
      "username": "gamma",
      "account_status": "running",
      "pid": 3003,
      "creation_time": "200",
      "cpu_percent": 18.0,
      "working_set_bytes": 268435456,
      "private_memory_bytes": null,
      "gpu_percent": 9.5,
      "gpu_availability": "available",
      "uptime_seconds": 60,
      "sampled_at": "2026-11-13T00:00:00Z",
      "availability": "available"
    }
  ],
  "metadata": {
    "cpu_normalization": "machine_percent",
    "cpu_formula": "delta_cpu_time / (elapsed_wall_time * logical_processors) * 100",
    "memory_units": "bytes",
    "gpu_normalization": "gpu_engine_percent_sum_capped_100",
    "sample_interval_ms": 1000,
    "history_capacity": 60,
    "stale_threshold_ms": 5000,
    "sampling_mode": "active",
    "last_sample_duration_ms": 4
  },
  "history": [
    {
      "timestamp": "2026-11-13T00:00:00Z",
      "system_cpu_percent": 31.25,
      "ram_utilization_percent": 50.0,
      "managed_cpu_percent": 12.5,
      "managed_ram_bytes": 536870912,
      "managed_gpu_percent": null,
      "running_instances": 1
    }
  ]
}
""";
var parsedResources = JsonSerializer.Deserialize<ResourceSnapshotResponse>(resourceJson, new JsonSerializerOptions { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower, PropertyNameCaseInsensitive = true })!;
Check(parsedResources.Metadata.CpuNormalization == "machine_percent" && parsedResources.Aggregates.RunningInstances == 1, "Resource snapshot deserializes normalized CPU and aggregate counts.");
Check(ResourceFormatting.FormatBytes(536870912) == "512.0 MB" && ResourceFormatting.FormatBytes(16UL * 1024 * 1024 * 1024) == "16.00 GB", "IEC memory formatting renders MB and GB consistently.");
Check(ResourceFormatting.FormatPercentMetric(null, "unavailable") == "Unavailable" && ResourceFormatting.FormatPercentMetric(null, "pending") == "Sampling…", "Unavailable and pending resource metrics render explicit states.");
var byCpuDesc = ResourceFormatting.SortInstances(parsedResources.Accounts, "cpu", true);
Check(byCpuDesc.Select(a => a.AccountId).SequenceEqual(["3", "1", "2"]), "Sorting by CPU descending puts highest live instance first and nulls last.");
var byRamAsc = ResourceFormatting.SortInstances(parsedResources.Accounts, "ram", false);
Check(byRamAsc.Select(a => a.AccountId).SequenceEqual(["3", "1", "2"]), "Sorting by RAM ascending keeps non-null rows before exited/null rows.");
var alphaRow = new ResourceInstanceRow(parsedResources.Accounts[0]);
var betaRow = new ResourceInstanceRow(parsedResources.Accounts[1]);
var gammaRow = new ResourceInstanceRow(parsedResources.Accounts[2]);
Check(alphaRow.Cpu == "12.5%" && alphaRow.Gpu == "Unavailable" && alphaRow.Uptime == "1h 1m 5s", "Available instance row formats CPU, unavailable GPU, and uptime.");
Check(betaRow.Cpu == "—" && betaRow.Status.Contains("Exited"), "Exited instance row shows exited availability and placeholder metrics.");
Check(gammaRow.Gpu == "9.5%", "Optional GPU utilization formats when supported.");
var history = ResourceFormatting.MergeHistory([], parsedResources, 60);
var nextPoint = parsedResources with { Timestamp = parsedResources.Timestamp.AddSeconds(1), History = null };
for (var s = 1; s <= 65; s++)
    history = ResourceFormatting.MergeHistory(history, parsedResources with { Timestamp = parsedResources.Timestamp.AddSeconds(s), History = null }, 60);
Check(history.Length == 60 && history[0].Timestamp == parsedResources.Timestamp.AddSeconds(6), "Client history buffer stays bounded at 60 samples.");

await UpdateDownloadTests.RunAsync(Check);
await ImportTests.RunAsync(Check);
Console.WriteLine($"{passed} desktop model/editor/update checks passed.");
