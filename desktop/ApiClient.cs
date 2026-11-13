using System.Net.Http;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;

namespace RoLauncher.Desktop;

public sealed class ApiClient : IDisposable
{
    public static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        PropertyNameCaseInsensitive = true
    };
    private readonly HttpClient _client;
    public ApiClient(Bootstrap config)
    {
        _client = new(new HttpClientHandler { AllowAutoRedirect = false, UseProxy = false })
        {
            BaseAddress = new Uri($"http://127.0.0.1:{config.Port}/v1/"),
            Timeout = Timeout.InfiniteTimeSpan
        };
        _client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", config.Token);
    }
    public async Task<T> GetAsync<T>(string path)
    {
        var content = await SendAsync(HttpMethod.Get, path);
        if (content is null) throw new ApiException("RoLauncher returned an empty response. Try again.");
        try { return content.Value.Deserialize<T>(JsonOptions) ?? throw new ApiException("RoLauncher returned an empty response. Try again."); }
        catch (JsonException) { throw new ApiException("RoLauncher returned an invalid response. Try again."); }
    }
    public async Task<JsonElement?> SendAsync(HttpMethod method, string path, object? body = null)
    {
        using var request = new HttpRequestMessage(method, path);
        // A full cookie batch may need thirteen 20-second validation waves.
        using var deadline = new CancellationTokenSource(method == HttpMethod.Post && path == "accounts"
            ? TimeSpan.FromMinutes(10) : path == "instances/close-all" ? TimeSpan.FromMinutes(5) : TimeSpan.FromSeconds(30));
        if (body is not null)
            request.Content = new StringContent(JsonSerializer.Serialize(body, JsonOptions), Encoding.UTF8, "application/json");
        using var response = await _client.SendAsync(request, deadline.Token);
        var content = await response.Content.ReadAsStringAsync(deadline.Token);
        if (!response.IsSuccessStatusCode)
        {
            string error = $"Request failed (HTTP {(int)response.StatusCode}).";
            try
            {
                using var json = JsonDocument.Parse(content);
                if (json.RootElement.ValueKind == JsonValueKind.Object &&
                    json.RootElement.TryGetProperty("error", out var message) && message.ValueKind == JsonValueKind.String)
                    error = message.GetString() ?? error;
            }
            catch (JsonException) { }
            throw new ApiException(error);
        }
        if (content.Length == 0) return null;
        try
        {
            var result = JsonSerializer.Deserialize<JsonElement>(content);
            if (result.ValueKind == JsonValueKind.Null)
                throw new ApiException("RoLauncher returned an empty response. Try again.");
            return result;
        }
        catch (JsonException) { throw new ApiException("RoLauncher returned an invalid response. Try again."); }
    }
    public async IAsyncEnumerable<ResourceSnapshotResponse> StreamResourcesAsync(
        [EnumeratorCancellation] CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, "resources/events");
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("text/event-stream"));
        using var response = await _client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        if (!response.IsSuccessStatusCode)
            throw new ApiException($"Resource stream unavailable (HTTP {(int)response.StatusCode}).");
        using var stream = await response.Content.ReadAsStreamAsync(cancellationToken);
        using var reader = new StreamReader(stream);
        var data = new StringBuilder();
        while (!cancellationToken.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync(cancellationToken);
            if (line is null) yield break;
            if (line.Length == 0)
            {
                if (data.Length == 0) continue;
                var json = data.ToString().TrimEnd();
                data.Clear();
                ResourceSnapshotResponse? parsed = null;
                try { parsed = JsonSerializer.Deserialize<ResourceSnapshotResponse>(json, JsonOptions); }
                catch (JsonException) { }
                if (parsed is not null) yield return parsed;
            }
            else if (line.StartsWith("data:", StringComparison.Ordinal))
            {
                if (data.Length > 0) data.Append('\n');
                data.Append(line[5..].TrimStart());
            }
        }
    }
    public void Dispose() => _client.Dispose();
}
