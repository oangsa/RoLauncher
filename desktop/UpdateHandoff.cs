using System.Diagnostics;
using System.Text.Json;

namespace RoLauncher.Desktop;

internal static class UpdateHandoff
{
    public static async Task PrepareAsync(DownloadedInstaller installer, Bootstrap config, CancellationToken cancellation)
    {
        await UpdateDownloader.VerifyAsync(installer, cancellation);
        using var parent = Process.GetProcessById(config.ParentId);
        using var desktop = Process.GetCurrentProcess();
        var executable = parent.MainModule?.FileName ?? throw new InvalidOperationException("Cannot locate the supervisor.");
        if (!string.Equals(Path.GetFileName(executable), "RoLauncher.exe", StringComparison.OrdinalIgnoreCase))
            throw new InvalidOperationException("Start RoLauncher using its launcher before applying an update.");
        if (string.IsNullOrWhiteSpace(config.DataDirectory)) throw new InvalidOperationException("The data directory is unavailable.");
        var directory = Path.Combine(Path.GetDirectoryName(installer.Path)!, "handoff-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        var helper = Path.Combine(directory, "apply-update.ps1");
        using (var script = typeof(UpdateHandoff).Assembly.GetManifestResourceStream("RoLauncher.ApplyUpdate.ps1")!)
        await using (var output = File.Create(helper)) { await script.CopyToAsync(output, cancellation); }
        var manifest = Path.Combine(directory, "request.json");
        await File.WriteAllTextAsync(manifest, JsonSerializer.Serialize(new
        {
            Installer = installer.Path, installer.Sha256, installer.Version,
            InstallDirectory = Path.GetDirectoryName(executable), DataDirectory = Path.GetFullPath(config.DataDirectory), config.Port,
            ParentId = parent.Id, ParentStarted = parent.StartTime.ToUniversalTime().Ticks,
            DesktopId = desktop.Id, DesktopStarted = desktop.StartTime.ToUniversalTime().Ticks
        }), cancellation);
        var start = new ProcessStartInfo(Path.Combine(Environment.SystemDirectory, "WindowsPowerShell", "v1.0", "powershell.exe"))
        { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
        foreach (var argument in new[] { "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", helper, "-Manifest", manifest })
            start.ArgumentList.Add(argument);
        using var worker = Process.Start(start) ?? throw new InvalidOperationException("Cannot start the installer helper.");
        var deadline = DateTime.UtcNow.AddSeconds(15);
        while (!File.Exists(Path.Combine(directory, "ready")))
        {
            cancellation.ThrowIfCancellationRequested();
            if (worker.HasExited || DateTime.UtcNow >= deadline)
                throw new InvalidOperationException("The update helper could not prepare. RoLauncher has stayed open.");
            await Task.Delay(100, cancellation);
        }
        await File.WriteAllTextAsync(Path.Combine(directory, "commit"), "Apply", cancellation);
    }
}
