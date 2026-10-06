using System.Diagnostics;
using System.Reflection;
using System.Text.Json;

namespace RoLauncher.Desktop;

public static class UpdateHandoff
{
    public static async Task PrepareAsync(DownloadedInstaller package, Bootstrap config, CancellationToken cancellation)
    {
        await UpdateDownloader.VerifyAsync(package, cancellation);
        using var resource = Assembly.GetExecutingAssembly().GetManifestResourceStream("RoLauncher.ApplyUpdate.Linux.py")
            ?? throw new InvalidOperationException("Linux updater is missing.");
        using var reader = new StreamReader(resource);
        var script = await reader.ReadToEndAsync(cancellation);
        var root = Directory.GetParent(AppContext.BaseDirectory.TrimEnd(Path.DirectorySeparatorChar))!.FullName;
        if (!File.Exists(Path.Combine(root, "RoLauncher"))) throw new InvalidOperationException("Supervisor is missing.");
        var start = new ProcessStartInfo("python3") { UseShellExecute = false, RedirectStandardInput = true,
            RedirectStandardOutput = true, RedirectStandardError = true };
        start.ArgumentList.Add("-c"); start.ArgumentList.Add(script);
        using var worker = Process.Start(start) ?? throw new InvalidOperationException("Cannot start Linux updater.");
        try
        {
            // No bearer token, cookie or authenticated URL is sent to this helper.
            await worker.StandardInput.WriteLineAsync(JsonSerializer.Serialize(new {
                archive = package.Path, hash = package.Sha256, version = package.Version,
                root, data = config.DataDirectory, port = config.Port,
                parent = config.ParentId, desktop = Environment.ProcessId
            }).AsMemory(), cancellation);
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
            timeout.CancelAfter(TimeSpan.FromMinutes(2));
            var result = await worker.StandardOutput.ReadLineAsync(timeout.Token);
            if (result != "ready") throw new InvalidOperationException("Cannot prepare Linux update.");
            await worker.StandardInput.WriteLineAsync("commit".AsMemory(), cancellation);
            worker.StandardInput.Close();
        }
        catch { if (!worker.HasExited) worker.Kill(); throw; }
    }
}
