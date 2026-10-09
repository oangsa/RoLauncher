using System.Diagnostics;
using System.Text.Json;
using Microsoft.UI.Xaml;

namespace RoLauncher.Desktop;

public partial class App : Application
{
    private MainWindow? _window;

    public App()
    {
        Uno.UI.FeatureConfiguration.TextBox.UseOverlayOnSkia = false;
        InitializeComponent();
        Resources["ContentControlThemeFontFamily"] = new Microsoft.UI.Xaml.Media.FontFamily(
            "ms-appx:///Uno.Fonts.OpenSans/Fonts/OpenSans.ttf");
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Bootstrap? config = null;
        try { config = JsonSerializer.Deserialize<Bootstrap>(Console.ReadLine() ?? "", ApiClient.JsonOptions); }
        catch (JsonException) { }
        if (config is null || config.Port is < 1 or > 65535 || config.Token.Length < 32 || config.ParentId <= 0)
        {
            Console.Error.WriteLine("Start RoLauncher using the RoLauncher supervisor.");
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
