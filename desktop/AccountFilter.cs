namespace RoLauncher.Desktop;

public static class AccountFilter
{
    public static bool Matches(Account account, string search, string status, string rejoin)
    {
        var name = search.Trim();
        var nameMatches = name.Length == 0 || account.Alias.Contains(name, StringComparison.OrdinalIgnoreCase) ||
            account.Username.Contains(name.TrimStart('@'), StringComparison.OrdinalIgnoreCase);
        return nameMatches && (status.Length == 0 || account.Status == status) &&
            (rejoin.Length == 0 || account.AutoRecovery == (rejoin == "on"));
    }
}
