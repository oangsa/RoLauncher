using System.Net.Http;
using System.Text.Json;

namespace RoLauncher.Desktop;

internal static class CookieBatch
{
    public static string[] Parse(string text) => text.Split(['\r', '\n'], StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries)
        .Distinct(StringComparer.Ordinal).ToArray();

    internal sealed record Result(int Imported, IReadOnlyList<string> Errors);

    public static async Task<Result> ImportAsync(string[] cookies, Func<string[], Task<JsonElement?>> send)
    {
        int imported = 0; var errors = new List<string>();
        try
        {
            if (cookies.Length == 0 || cookies.All(string.IsNullOrWhiteSpace))
                return new(0, ["Paste a session cookie first."]);
            for (int offset = 0; offset < cookies.Length; offset += 50)
            {
                var batch = cookies.AsSpan(offset, Math.Min(50, cookies.Length - offset)).ToArray();
                try
                {
                    var response = await send(batch);
                    if (response is not { ValueKind: JsonValueKind.Object } value ||
                        !value.TryGetProperty("accounts", out var accounts) || accounts.ValueKind != JsonValueKind.Array ||
                        !value.TryGetProperty("errors", out var failures) || failures.ValueKind != JsonValueKind.Array ||
                        accounts.GetArrayLength() + failures.GetArrayLength() != batch.Length ||
                        accounts.EnumerateArray().Any(a => a.ValueKind != JsonValueKind.Object))
                        throw new JsonException();
                    var indexed = new SortedDictionary<int, string>();
                    foreach (var failure in failures.EnumerateArray())
                    {
                        if (failure.ValueKind != JsonValueKind.Object ||
                            !failure.TryGetProperty("index", out var index) || index.ValueKind != JsonValueKind.Number ||
                            !index.TryGetInt32(out int position) || position < 0 || position >= batch.Length ||
                            !failure.TryGetProperty("error", out var message) || message.ValueKind != JsonValueKind.String ||
                            !indexed.TryAdd(position, message.GetString() ?? "Import failed."))
                            throw new JsonException();
                    }
                    imported += accounts.GetArrayLength();
                    errors.AddRange(indexed.Select(e => $"Entry {offset + e.Key + 1}: {e.Value}"));
                }
                catch (Exception ex) when (ex is ApiException or HttpRequestException or OperationCanceledException or JsonException or ObjectDisposedException)
                {
                    var reason = ex switch {
                        OperationCanceledException => "Request timed out or was cancelled.",
                        JsonException => "RoLauncher returned an invalid import response.",
                        _ => "Import request failed."
                    };
                    errors.Add($"Import stopped at entry {offset + 1}. {reason} Some accounts may have saved; check the refreshed list before retrying.");
                    break;
                }
                finally { Array.Clear(batch); }
            }
            return new(imported, errors);
        }
        finally { Array.Clear(cookies); }
    }
}

public sealed class ApiException(string message) : Exception(message);
