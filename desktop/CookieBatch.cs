namespace RoLauncher.Desktop;

internal static class CookieBatch
{
    public static string[] Parse(string text) => text.Split(['\r', '\n'], StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries)
        .Distinct(StringComparer.Ordinal).ToArray();
}
