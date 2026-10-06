using System.Net;
using System.Security.Cryptography;
using RoLauncher.Desktop;

internal static class UpdateDownloadTests
{
    public static async Task RunAsync(Action<bool, string> check)
    {
        var linux = OperatingSystem.IsLinux();
        var name = "rolauncher-v1.0.1-" + (linux ? "linux-x64.tar.gz" : "setup-x64.exe");
        var prefix = UpdateDownloader.Publisher + "/releases/download/v1.0.1/";
        var view = new UpdateView(true, "1.0.1", UpdateDownloader.Publisher + "/releases/tag/v1.0.1",
            prefix + (linux ? name : "rolauncher-v1.0.1-windows-x64.zip"), prefix + "rolauncher-v1.0.1-SHA256SUMS.txt", linux ? "" : prefix + name);
        check(UpdateDownloader.ValidateRelease(view, "1.0.0") == name, "A newer official installer is accepted.");
        void Rejected(Action action, string description)
        {
            try { action(); check(false, description); } catch (InvalidDataException) { check(true, description); }
        }
        Rejected(() => UpdateDownloader.ValidateRelease(view, "1.0.1"), "Same-version installs are rejected.");
        Rejected(() => UpdateDownloader.ValidateRelease(view, "2.0.0"), "Downgrades are rejected.");
        Rejected(() => UpdateDownloader.ValidateRelease(linux ? view with { DownloadUrl = "https://evil.example/package.tar.gz" }
            : view with { InstallerUrl = "https://evil.example/setup.exe" }, "1.0.0"), "Foreign platform packages are rejected.");
        Rejected(() => UpdateDownloader.ValidateRelease(view with { Version = "1.0.1-beta" }, "1.0.0"), "Prerelease metadata is rejected.");
        check(!UpdateDownloader.AllowedRedirect(new("https://github.com.evil.example/a")) && !UpdateDownloader.AllowedRedirect(new("http://release-assets.githubusercontent.com/a")), "Redirects cannot escape GitHub or downgrade HTTPS.");
        var payload = linux ? new byte[] { 0x1f, 0x8b, 0x08, 0x00, 1, 2, 3 } : "MZisolated-installer-fixture"u8.ToArray();
        var hash = Convert.ToHexString(SHA256.HashData(payload)).ToLowerInvariant();
        Rejected(() => UpdateDownloader.ReadChecksum($"{hash}  {name}\n{hash}  {name}", name), "Duplicate checksum entries are rejected.");
        Rejected(() => UpdateDownloader.ReadChecksum($"bad  {name}", name), "Malformed installer digests are rejected.");
        Rejected(() => UpdateDownloader.ReadChecksum($"{hash}  other.exe", name), "Missing installer digests are rejected.");
        var root = Path.Combine(Path.GetTempPath(), "rolauncher-update-tests-" + Guid.NewGuid().ToString("N"));
        try
        {
            Directory.CreateDirectory(root);
            var wrongPlatform = linux ? "MZwindows-fixture"u8.ToArray() : new byte[] { 0x1f, 0x8b, 0x08, 1, 2, 3 };
            var wrongPath = Path.Combine(root, "wrong-platform");
            await File.WriteAllBytesAsync(wrongPath, wrongPlatform);
            try { await UpdateDownloader.VerifyAsync(new(wrongPath, Convert.ToHexString(SHA256.HashData(wrongPlatform)), "1.0.1")); check(false, "Wrong-platform file rejected."); }
            catch (InvalidDataException) { check(true, "A matching checksum cannot authorize the wrong platform's file."); }
            var requests = new List<Uri>();
            using var client = new HttpClient(new Fixture(request =>
            {
                check(request.Headers.Authorization is null && !request.Headers.Contains("Cookie"), "Update requests never contain API tokens or cookies.");
                requests.Add(request.RequestUri!);
                if (request.RequestUri!.AbsolutePath.EndsWith("SHA256SUMS.txt")) return new(HttpStatusCode.OK) { Content = new StringContent($"{hash}  {name}\n") };
                if (request.RequestUri.Host == "github.com") return new(HttpStatusCode.Found) { Headers = { Location = new Uri("https://release-assets.githubusercontent.com/fixture") } };
                return new(HttpStatusCode.OK) { Content = new ByteArrayContent(payload) };
            }));
            using var downloader = new UpdateDownloader(Path.Combine(root, "valid"), client);
            var progress = new InlineProgress();
            var ready = await downloader.DownloadAsync(view, "1.0.0", progress, CancellationToken.None);
            check(File.ReadAllBytes(ready.Path).SequenceEqual(payload) && progress.Last == 100, "The redirected installer is saved only after verification.");
            check(requests.Any(u => u.Host == "release-assets.githubusercontent.com"), "The official GitHub asset redirect works.");
            check(await downloader.CachedAsync(view, "1.0.0", CancellationToken.None) is not null, "A cached download is reverified against the current official checksum.");
            await File.WriteAllTextAsync(ready.Path, "MZcorrupted");
            check(await downloader.CachedAsync(view, "1.0.0", CancellationToken.None) is null && !File.Exists(ready.Path), "A corrupted cached installer is discarded.");

            async Task DownloadRejected(Func<HttpRequestMessage, HttpResponseMessage> reply, string scenario)
            {
                var cache = Path.Combine(root, scenario);
                using var failingClient = new HttpClient(new Fixture(reply));
                using var failing = new UpdateDownloader(cache, failingClient);
                try { await failing.DownloadAsync(view, "1.0.0", new InlineProgress(), CancellationToken.None); check(false, scenario); }
                catch (InvalidDataException) { check(!Directory.Exists(cache) || !Directory.EnumerateFiles(cache).Any(), scenario + " leaves no executable or partial file."); }
            }
            await DownloadRejected(request => request.RequestUri!.AbsolutePath.EndsWith("SHA256SUMS.txt")
                ? new(HttpStatusCode.OK) { Content = new StringContent($"{hash}  {name}\n") }
                : new(HttpStatusCode.OK) { Content = new ByteArrayContent("MZwrong-file"u8.ToArray()) }, "Checksum mismatch");
            await DownloadRejected(request => request.RequestUri!.AbsolutePath.EndsWith("SHA256SUMS.txt")
                ? new(HttpStatusCode.OK) { Content = new StringContent($"{hash}  {name}\n") }
                : new(HttpStatusCode.Found) { Headers = { Location = new Uri("https://evil.example/installer") } }, "Untrusted redirect");
            await DownloadRejected(request =>
            {
                if (request.RequestUri!.AbsolutePath.EndsWith("SHA256SUMS.txt")) return new(HttpStatusCode.OK) { Content = new StringContent($"{hash}  {name}\n") };
                var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(payload) };
                response.Content.Headers.ContentLength = UpdateDownloader.MaximumInstallerBytes + 1;
                return response;
            }, "Oversized installer");
            await DownloadRejected(_ => new(HttpStatusCode.OK) { Content = new ByteArrayContent([0xff, 0xfe, 0xff]) }, "Invalid checksum encoding");
            using var cancelled = new CancellationTokenSource(); cancelled.Cancel();
            try { await downloader.DownloadAsync(view, "1.0.0", new InlineProgress(), cancelled.Token); check(false, "Cancelled update"); }
            catch (OperationCanceledException) { check(!Directory.EnumerateFiles(Path.Combine(root, "valid")).Any(), "Cancellation never leaves an installer ready to execute."); }
        }
        finally { if (Directory.Exists(root)) Directory.Delete(root, true); }
    }

    private sealed class Fixture(Func<HttpRequestMessage, HttpResponseMessage> reply) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        { cancellationToken.ThrowIfCancellationRequested(); return Task.FromResult(reply(request)); }
    }
    private sealed class InlineProgress : IProgress<int> { public int Last; public void Report(int value) => Last = value; }
}
