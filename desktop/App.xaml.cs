using System.Diagnostics;
using System.Text.Json;
using Microsoft.UI.Xaml;

namespace RoLauncher.Desktop;

public partial class App : Microsoft.UI.Xaml.Application
{
    private MainWindow? _window;
    public App()
    {
#if UI_SMOKE
        DebugSettings.IsXamlResourceReferenceTracingEnabled = true;
        DebugSettings.XamlResourceReferenceFailed += (sender, e) =>
            File.AppendAllText(Path.Combine(Environment.GetEnvironmentVariable("ROLAUNCHER_UI_SMOKE_DIR")!, "xaml.txt"), e.Message + "\n");
        UnhandledException += (sender, e) =>
        {
            var directory = Environment.GetEnvironmentVariable("ROLAUNCHER_UI_SMOKE_DIR")!;
            Directory.CreateDirectory(directory);
            File.WriteAllText(Path.Combine(directory, "result.txt"), $"FAILED: {e.Message}\n{e.Exception}\n{string.Join("; ", e.Exception.Data.Keys.Cast<object>().Select(k => $"{k}={e.Exception.Data[k]}"))}");
        };
#endif
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        // Credentials arrive through an inherited anonymous pipe, never process arguments or files.
        var input = Console.ReadLine();
        Bootstrap? config = null;
        try { config = JsonSerializer.Deserialize<Bootstrap>(input ?? "", ApiClient.JsonOptions); }
        catch (JsonException) { }
        if (config is null || config.Port is < 1 or > 65535 || config.Token.Length < 32)
        {
            NativeTray.Message("Start RoLauncher using RoLauncher.exe.");
            Exit();
            return;
        }
        _window = new MainWindow(config);
        _window.Activate();
        _ = WatchSupervisorAsync(config.ParentId);
    }

    private async Task WatchSupervisorAsync(int parentId)
    {
        try
        {
            using var parent = Process.GetProcessById(parentId);
            await parent.WaitForExitAsync();
        }
        catch (ArgumentException) { }
        _window?.DispatcherQueue.TryEnqueue(() => _window.SupervisorExited());
    }
}
