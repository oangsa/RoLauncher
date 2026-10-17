#if UI_SMOKE
namespace RoLauncher.Desktop;

public sealed partial class MainWindow
{
    private async Task ImportFailureSmokeAsync()
    {
        foreach (var cookie in new[] { "fixture-malformed", "fixture-failure" })
        {
            var sensitive = new[] { cookie };
            await ImportAsync(sensitive);
            Assert(ImportButton.IsEnabled && FileButton.IsEnabled && !_importing,
                "Malformed and failed import responses restore desktop controls.");
            Assert(sensitive[0] is null && Feedback.Message.Contains("Import stopped at entry 1") &&
                !Feedback.Message.Contains(cookie), "Failed imports clear input arrays and show useful feedback without credentials.");
        }
    }
}
#endif
