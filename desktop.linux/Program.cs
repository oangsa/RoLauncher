using Uno.UI.Hosting;

namespace RoLauncher.Desktop;

internal static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        // Fully Skia-rendered controls: never adopt GNOME or KDE widget themes.
        UnoPlatformHostBuilder.Create().App(() => new App()).UseX11().Build().Run();
    }
}
