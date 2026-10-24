using RoLauncher.Desktop;
using System.Text.Json;

internal static class ImportTests
{
    internal static async Task RunAsync(Action<bool, string> check)
    {
        var inputs = Enumerable.Range(1, 103).Select(i => $"fixture-{i}").ToArray();
        var batches = new List<string[]>();
        int offset = 0;
        var result = await CookieBatch.ImportAsync(inputs, batch => {
            check(batch.Length <= 50 && batch.SequenceEqual(Enumerable.Range(offset + 1, batch.Length).Select(i => $"fixture-{i}")), "Batch inputs retain order and stay within API limit.");
            batches.Add(batch); offset += batch.Length;
            return Task.FromResult<JsonElement?>(JsonSerializer.SerializeToElement(new {
                accounts = Enumerable.Range(0, batch.Length - 1).Select(_ => new { id = "saved" }),
                errors = new[] { new { index = batch.Length - 1, error = "Session expired" } }
            }));
        });
        check(batches.Select(b => b.Length).SequenceEqual([50, 50, 3]), "More than 50 inputs use bounded sequential batch requests.");
        check(result.Imported == 100 && result.Errors.Select(e => e.Split(':')[0]).SequenceEqual(["Entry 50", "Entry 100", "Entry 103"]), "Partial successes and global error positions span batches correctly.");
        check(inputs.All(v => v is null) && batches.All(b => b.All(v => v is null)), "All original and temporary cookie arrays clear after success.");

        foreach (var response in new[] {
            "null", "{}", "{\"accounts\":[],\"errors\":[]}",
            "{\"accounts\":[],\"errors\":[{\"index\":9,\"error\":\"bad\"}]}",
            "{\"accounts\":[],\"errors\":[{\"index\":0,\"error\":{}}]}"
        }) {
            var sensitive = new[] { "secret-not-for-diagnostics" };
            string[]? temporary = null;
            var failed = await CookieBatch.ImportAsync(sensitive, b => { temporary = b; return Task.FromResult<JsonElement?>(JsonSerializer.Deserialize<JsonElement>(response)); });
            check(failed.Imported == 0 && failed.Errors.Single().Contains("invalid import response") && !failed.Errors.Single().Contains("secret"), "Malformed response reports uncertainty without credentials.");
            check(sensitive[0] is null && temporary![0] is null, "Malformed response clears both input arrays.");
        }
        foreach (var error in new Exception[] { new TaskCanceledException(), new System.Net.Http.HttpRequestException(), new ApiException("secret-not-for-diagnostics") }) {
            var sensitive = Enumerable.Repeat("secret-not-for-diagnostics", 51).ToArray();
            int calls = 0; string[]? temporary = null;
            var failed = await CookieBatch.ImportAsync(sensitive, batch => {
                temporary = batch;
                if (++calls == 2) throw error;
                return Task.FromResult<JsonElement?>(JsonSerializer.SerializeToElement(new { accounts = batch.Select(_ => new { id = "saved" }), errors = Array.Empty<object>() }));
            });
            check(failed.Imported == 50 && failed.Errors.Single().StartsWith("Import stopped at entry 51.") && !failed.Errors.Single().Contains("secret"), "Request failure keeps earlier successes, maps position, and omits sensitive exception text.");
            check(sensitive.All(v => v is null) && temporary![0] is null, "Request failure clears input buffers.");
        }
        var duplicates = CookieBatch.Parse("same\nexpired\nsame\nother");
        check(duplicates.SequenceEqual(["same", "expired", "other"]), "Duplicate cookies retain first submitted position after parsing.");
        var ordered = await CookieBatch.ImportAsync(duplicates, _ => Task.FromResult<JsonElement?>(JsonSerializer.Deserialize<JsonElement>("{\"accounts\":[{}],\"errors\":[{\"index\":2,\"error\":\"last\"},{\"index\":1,\"error\":\"expired\"}]}")));
        check(ordered.Errors[0].StartsWith("Entry 2:") && ordered.Errors[1].StartsWith("Entry 3:"), "Unsorted API errors display in submission order.");
    }
}
