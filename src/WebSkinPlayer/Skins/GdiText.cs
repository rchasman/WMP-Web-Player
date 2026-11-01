using System.Runtime.InteropServices;

namespace WebSkinPlayer.Skins;

/// <summary>
/// Text widths as GDI lays out Segoe UI 9 pt (-12 px, normal weight): whole-pixel letter widths, no kerning.
/// WMP sized its playlist columns by these, measured on a runner; the font's ideal widths drift from them by
/// up to a pixel a letter.
/// </summary>
static class GdiText
{
    [StructLayout(LayoutKind.Sequential)]
    struct Size { public int Cx, Cy; }

    [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr hdc);
    [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr hdc, IntPtr obj);
    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr CreateFontW(int height, int width, int escapement, int orientation, int weight, uint italic, uint underline,
        uint strikeOut, uint charSet, uint outPrecision, uint clipPrecision, uint quality, uint pitchAndFamily, string face);
    [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
    static extern bool GetTextExtentPoint32W(IntPtr hdc, string text, int length, out Size size);

    const int Normal = 400, DefaultCharset = 1;

    static readonly IntPtr Dc = Open();
    static readonly Dictionary<string, int> Widths = new();

    static IntPtr Open()
    {
        var dc = CreateCompatibleDC(IntPtr.Zero);
        SelectObject(dc, CreateFontW(-12, 0, 0, 0, Normal, 0, 0, 0, DefaultCharset, 0, 0, 0, 0, "Segoe UI"));
        return dc;
    }

    public static int Width(string s)
    {
        if (s.Length == 0) return 0;
        if (Widths.TryGetValue(s, out int w)) return w;
        GetTextExtentPoint32W(Dc, s, s.Length, out var size);
        return Widths[s] = size.Cx;
    }

    /// <summary>GDI's end ellipsis: the longest start of <paramref name="s"/> that fits <paramref name="room"/> with "..." after it.</summary>
    public static string EndEllipsis(string s, double room)
    {
        if (Width(s) <= room) return s;
        int kept = 0;
        while (kept < s.Length && Width(s[..(kept + 1)]) + Width("...") <= room) kept++;
        return s[..kept] + "...";
    }
}
