namespace RoLauncher.Desktop;

public sealed record Bootstrap(int Port, string Token, string Version, int ParentId, string DataDirectory = "");
