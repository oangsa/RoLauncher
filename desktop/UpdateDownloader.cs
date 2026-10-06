using System.Net;
using System.Net.Http;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace RoLauncher.Desktop;

public sealed record DownloadedInstaller(string Path, string Sha256, string Version);

/// Downloads only the official publisher's platform package; never forwards local API credentials.
public sealed class UpdateDownloader : IDisposable
{
    public const string Publisher = "https://github.com/oangsa/RoLauncher";
    public const long MaximumInstallerBytes = 256L * 1024 * 1024;
    private readonly HttpClient _http;
    private readonly string _cache;
    private readonly bool _ownsClient;

    public UpdateDownloader(string cache, HttpClient? client = null)
    {
        _cache = cache;
        _ownsClient = client is null;
        _http = client ?? new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false })
        { Timeout = TimeSpan.FromMinutes(15) };
    }

    public static string ValidateRelease(UpdateView view, string installedVersion)
    {
        if (!Regex.IsMatch(view.Version ?? "", @"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$") ||
            !Version.TryParse(view.Version, out var target) || !Version.TryParse(installedVersion, out var installed) ||
            !view.Available || target <= installed)
            throw new InvalidDataException("The update must be a newer version.");
        var name = $"rolauncher-v{view.Version}-" + (OperatingSystem.IsLinux() ? "linux-x64.tar.gz" : "setup-x64.exe");
        var tag = (view.Prerelease ? "beta-v" : "v") + view.Version;
        var prefix = $"{Publisher}/releases/download/{tag}/";
        var downloadUrl = OperatingSystem.IsLinux() ? view.DownloadUrl : view.InstallerUrl;
        if (downloadUrl != prefix + name || view.ChecksumsUrl != prefix + $"rolauncher-v{view.Version}-SHA256SUMS.txt" ||
            view.ReleaseUrl != $"{Publisher}/releases/tag/{tag}")
            throw new InvalidDataException("Automatic installation is supported only for official RoLauncher releases.");
        return name;
    }

    public static string ReadChecksum(string text, string filename)
    {
        string? result = null;
        foreach (var line in text.Split('\n'))
        {
            var fields = line.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
            if (fields.Length == 2 && fields[1] == filename)
            {
                if (result is not null || !Regex.IsMatch(fields[0], "^[a-fA-F0-9]{64}$"))
                    throw new InvalidDataException("The release checksum file is invalid.");
                result = fields[0].ToLowerInvariant();
            }
        }
        return result ?? throw new InvalidDataException("The installer checksum is missing.");
    }

    public static bool AllowedRedirect(Uri uri) => uri.Scheme == "https" && uri.Port == 443 &&
        uri.UserInfo.Length == 0 && uri.Fragment.Length == 0 &&
        uri.Host is "github.com" or "release-assets.githubusercontent.com" or "objects.githubusercontent.com";

    private async Task<HttpResponseMessage> OpenAsync(string url, CancellationToken cancellation)
    {
        var current = new Uri(url);
        for (var hop = 0; hop <= 5; hop++)
        {
            if (!AllowedRedirect(current)) throw new InvalidDataException("The update download redirected outside GitHub.");
            using var request = new HttpRequestMessage(HttpMethod.Get, current);
            request.Headers.UserAgent.ParseAdd("RoLauncher/" + typeof(UpdateDownloader).Assembly.GetName().Version!.ToString(3));
            var response = await _http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellation);
            if ((int)response.StatusCode is 301 or 302 or 303 or 307 or 308)
            {
                var location = response.Headers.Location;
                response.Dispose();
                if (location is null) throw new InvalidDataException("The update redirect is invalid.");
                current = new Uri(current, location);
                continue;
            }
            if (!response.IsSuccessStatusCode)
            {
                var code = (int)response.StatusCode;
                response.Dispose();
                throw new HttpRequestException($"The update download failed (HTTP {code}).");
            }
            return response;
        }
        throw new InvalidDataException("The update download redirected too many times.");
    }

    private async Task<string> ExpectedHashAsync(UpdateView view, string name, CancellationToken cancellation)
    {
        using var response = await OpenAsync(view.ChecksumsUrl, cancellation);
        if (response.Content.Headers.ContentLength > 65536) throw new InvalidDataException("The checksum file is too large.");
        using var stream = await response.Content.ReadAsStreamAsync(cancellation);
        using var bytes = new MemoryStream();
        var buffer = new byte[8192];
        int length;
        while ((length = await stream.ReadAsync(buffer, cancellation)) > 0)
        {
            if (bytes.Length + length > 65536) throw new InvalidDataException("The checksum file is too large.");
            bytes.Write(buffer, 0, length);
        }
        try { return ReadChecksum(new UTF8Encoding(false, true).GetString(bytes.ToArray()), name); }
        catch (DecoderFallbackException) { throw new InvalidDataException("The checksum file is not valid UTF-8."); }
    }

    public static async Task VerifyAsync(DownloadedInstaller installer, CancellationToken cancellation = default)
    {
        await using var stream = File.OpenRead(installer.Path);
        var linux = OperatingSystem.IsLinux();
        if (stream.Length < 2 || stream.Length > MaximumInstallerBytes ||
            stream.ReadByte() != (linux ? 0x1f : 'M') || stream.ReadByte() != (linux ? 0x8b : 'Z'))
            throw new InvalidDataException(linux ? "The downloaded file is not a Linux package." : "The downloaded file is not a Windows installer.");
        stream.Position = 0;
        var actual = Convert.ToHexString(await SHA256.HashDataAsync(stream, cancellation));
        if (!string.Equals(actual, installer.Sha256, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException("The installer checksum did not match. Download the update again.");
    }

    public async Task<DownloadedInstaller?> CachedAsync(UpdateView view, string installedVersion, CancellationToken cancellation)
    {
        var name = ValidateRelease(view, installedVersion);
        var file = System.IO.Path.Combine(_cache, name);
        if (!File.Exists(file)) return null;
        var result = new DownloadedInstaller(file, await ExpectedHashAsync(view, name, cancellation), view.Version);
        try { await VerifyAsync(result, cancellation); return result; }
        catch (InvalidDataException) { File.Delete(file); return null; }
    }

    public async Task<DownloadedInstaller> DownloadAsync(UpdateView view, string installedVersion, IProgress<int> progress,
        CancellationToken cancellation)
    {
        var name = ValidateRelease(view, installedVersion);
        var hash = await ExpectedHashAsync(view, name, cancellation);
        Directory.CreateDirectory(_cache);
        var destination = System.IO.Path.Combine(_cache, name);
        var partial = destination + "." + Guid.NewGuid().ToString("N") + ".partial";
        try
        {
            using var response = await OpenAsync(OperatingSystem.IsLinux() ? view.DownloadUrl : view.InstallerUrl, cancellation);
            var total = response.Content.Headers.ContentLength;
            if (total > MaximumInstallerBytes) throw new InvalidDataException("The installer download is too large.");
            await using (var input = await response.Content.ReadAsStreamAsync(cancellation))
            await using (var output = new FileStream(partial, FileMode.CreateNew, FileAccess.Write, FileShare.None, 81920, true))
            {
                var buffer = new byte[81920];
                long received = 0;
                int length;
                while ((length = await input.ReadAsync(buffer, cancellation)) > 0)
                {
                    received += length;
                    if (received > MaximumInstallerBytes) throw new InvalidDataException("The installer download is too large.");
                    await output.WriteAsync(buffer.AsMemory(0, length), cancellation);
                    progress.Report(total is > 0 ? (int)Math.Min(99, received * 100 / total.Value) : -1);
                }
                await output.FlushAsync(cancellation);
            }
            await VerifyAsync(new(partial, hash, view.Version), cancellation);
            File.Move(partial, destination, true);
            progress.Report(100);
            return new(destination, hash, view.Version);
        }
        finally { if (File.Exists(partial)) File.Delete(partial); }
    }

    public void Dispose() { if (_ownsClient) _http.Dispose(); }
}
