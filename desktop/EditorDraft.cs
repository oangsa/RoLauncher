using System.Globalization;

namespace RoLauncher.Desktop;

public sealed class EditorDraft
{
    public string Alias { get; set; } = "";
    public string Place { get; set; } = "";
    public string Job { get; set; } = "";
    public string Private { get; set; } = "";
    public static EditorDraft From(Account a) => new()
    {
        Alias = a.Alias, Place = a.Target?.PlaceId.ToString(CultureInfo.InvariantCulture) ?? "",
        Job = a.Target?.JobId ?? "", Private = a.Target?.PrivateServerLink ?? ""
    };
    public bool TryTarget(out Target? target, out string error)
    {
        target = null;
        error = "Enter a valid Place ID greater than zero.";
        if (!ulong.TryParse(Place.Trim(), NumberStyles.None, CultureInfo.InvariantCulture, out var place) || place == 0) return false;
        var job = Job.Trim(); var link = Private.Trim();
        if (job.Length != 0 && link.Length != 0) { error = "Choose a Job ID or a private server link."; return false; }
        if (job.Length != 0 && !Guid.TryParseExact(job, "D", out _)) { error = "Enter a valid server Job ID."; return false; }
        if (link.Length != 0)
        {
            error = "Enter a valid Roblox private server link matching the Place ID.";
            if (link.Length > 2048 || !Uri.TryCreate(link, UriKind.Absolute, out var uri) || uri.Scheme != "https" ||
                uri.Host is not ("www.roblox.com" or "roblox.com") || uri.UserInfo.Length != 0 || !uri.IsDefaultPort) return false;
            var query = new Dictionary<string, string>();
            foreach (var pair in uri.Query.TrimStart('?').Split('&', StringSplitOptions.RemoveEmptyEntries))
            {
                var parts = pair.Split('=', 2);
                query[Uri.UnescapeDataString(parts[0])] = parts.Length == 2 ? Uri.UnescapeDataString(parts[1]) : "";
            }
            if (query.ContainsKey("code") && (!query.TryGetValue("type", out var kind) || kind != "Server")) return false;
            if (query.TryGetValue("privateServerLinkCode", out var privateCode))
            {
                var path = uri.AbsolutePath.Split('/');
                if (path.Length < 3 || path[1] != "games" || !ulong.TryParse(path[2], out var game) || game != place) return false;
            }
            var code = privateCode ?? query.GetValueOrDefault("code");
            if (string.IsNullOrEmpty(code) || code.Length > 512 || !code.All(c => char.IsAsciiLetterOrDigit(c) || c == '-')) return false;
        }
        target = new Target(place, job.Length == 0 ? null : job, link.Length == 0 ? null : link);
        error = "";
        return true;
    }
}
