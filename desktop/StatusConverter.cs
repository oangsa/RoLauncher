using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;
using Windows.UI;

namespace RoLauncher.Desktop;

public sealed class StatusConverter : IValueConverter
{
    public bool Background { get; set; }
    public object Convert(object value, Type targetType, object parameter, string language)
    {
        var colors = (value as string) switch
        {
            "running" => (0xEAF6EEu, 0x267346u),
            "needs_attention" => (0xFDEDECu, 0xB42318u),
            "reconnecting" or "backoff" or "unknown" => (0xFFF5DEu, 0x946200u),
            "launching" or "queued" => (0xEBF2FFu, 0x315CA8u),
            _ => (0xF0F0F2u, 0x62626Au)
        };
        var c = Background ? colors.Item1 : colors.Item2;
        return new SolidColorBrush(Color.FromArgb(255, (byte)(c >> 16), (byte)(c >> 8), (byte)c));
    }
    public object ConvertBack(object value, Type targetType, object parameter, string language) => throw new NotSupportedException();
}
