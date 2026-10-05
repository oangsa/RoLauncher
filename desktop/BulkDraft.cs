namespace RoLauncher.Desktop;

public sealed class BulkDraft
{
    public EditorDraft Fields { get; set; } = new();
    public bool ApplyAlias { get; set; }
    public bool ApplyTarget { get; set; }
    public bool ClearTarget { get; set; }
    public bool ApplyRejoin { get; set; }
    public bool Rejoin { get; set; }
    public bool ApplyGroup { get; set; }
    public string Group { get; set; } = "";
    public bool ApplyFallback { get; set; }
    public string Fallback { get; set; } = "allow_public";
    public bool TryPatch(out Dictionary<string, object?> patch, out string error)
    {
        patch = []; error = "";
        if (ApplyAlias)
        {
            if (string.IsNullOrWhiteSpace(Fields.Alias) || Fields.Alias.Length > 100 || Fields.Alias.Any(char.IsControl))
            { error = "Enter an alias or alias pattern with up to 100 characters."; return false; }
            patch["alias"] = Fields.Alias;
        }
        if (ApplyTarget)
        {
            if (ClearTarget) patch["clear_target"] = true;
            else
            {
                if (!Fields.TryTarget(out var target, out error)) return false;
                patch["target"] = target;
            }
        }
        if (ApplyRejoin) patch["auto_recovery"] = Rejoin;
        if (ApplyGroup)
        {
            if (Group.Length > 100 || Group.Any(char.IsControl)) { error = "Group must be at most 100 characters without control characters."; return false; }
            patch["group"] = Group;
        }
        if (ApplyFallback) patch["fallback_policy"] = Fallback;
        if (patch.Count == 0) { error = "Choose at least one Apply setting."; return false; }
        return true;
    }
}
