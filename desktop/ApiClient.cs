using System.Net.Http;
using System.Net.Http.Headers;
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
            Timeout = TimeSpan.FromSeconds(30)
        };
        _client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", config.Token);
    }
    public async Task<T> GetAsync<T>(string path) =>
        (await SendAsync(HttpMethod.Get, path))!.Value.Deserialize<T>(JsonOptions)!;
    public async Task<JsonElement?> SendAsync(HttpMethod method, string path, object? body = null)
    {
        using var request = new HttpRequestMessage(method, path);
        if (body is not null)
            request.Content = new StringContent(JsonSerializer.Serialize(body, JsonOptions), Encoding.UTF8, "application/json");
        using var response = await _client.SendAsync(request);
        var content = await response.Content.ReadAsStringAsync();
        if (!response.IsSuccessStatusCode)
        {
            string error = $"Request failed (HTTP {(int)response.StatusCode}).";
            try
            {
                using var json = JsonDocument.Parse(content);
                if (json.RootElement.TryGetProperty("error", out var message)) error = message.GetString() ?? error;
            }
            catch (JsonException) { }
            throw new ApiException(error);
        }
        return content.Length == 0 ? null : JsonSerializer.Deserialize<JsonElement>(content);
    }
    public void Dispose() => _client.Dispose();
}

public sealed class ApiException(string message) : Exception(message);
