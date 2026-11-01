using System.Globalization;
using System.Windows;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>TEXT, STATUSTEXT, CURRENTPOSITIONTEXT and friends.</summary>
public sealed class TextElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["value"] = PType.String, ["foregroundcolor"] = PType.String, ["backgroundcolor"] = PType.String,
        ["hoverforegroundcolor"] = PType.String, ["hoverbackgroundcolor"] = PType.String,
        ["disabledforegroundcolor"] = PType.String, ["fontface"] = PType.String, ["fontsize"] = PType.Number,
        ["fontstyle"] = PType.String, ["hoverfontstyle"] = PType.String, ["disabledfontstyle"] = PType.String,
        ["fontsmoothing"] = PType.Bool, ["fonttype"] = PType.String, ["justification"] = PType.String,
        ["scrolling"] = PType.Bool, ["scrollingdelay"] = PType.Int, ["scrollingamount"] = PType.Int,
        ["scrollingdirection"] = PType.String, ["wordwrap"] = PType.Bool, ["truncate"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    double _scroll;
    FormattedText? _cache;
    string _cacheKey = "";

    public TextElement(SkinView view, WmsNode node) : base(view, node)
    {
        Computed("textWidth", () => (double)Math.Ceiling(Measure().WidthIncludingTrailingWhitespace));
        Computed("textHeight", () => (double)Math.Ceiling(Measure().Height));
    }

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "foregroundcolor" => "#FFFFFF",
        "backgroundcolor" => "none",
        "fontface" => "Tahoma",
        // WMP's docs give TEXT.fontSize a default of 10 points.
        "fontsize" => 10.0,
        "fontstyle" => "normal",
        "fontsmoothing" => true,
        "justification" => "left",
        "scrollingdelay" => 50.0,
        "scrollingamount" => 1.0,
        "scrollingdirection" => "left",
        _ => null,
    };

    string Text => GetString("value");

    FormattedText Measure()
    {
        string style = GetString(!Enabled && GetString("disabledfontstyle").Length > 0 ? "disabledfontstyle"
            : IsHovered && GetString("hoverfontstyle").Length > 0 ? "hoverfontstyle" : "fontstyle").ToLowerInvariant();
        // Only an explicit width constrains wrapping (the natural width is derived from this measure).
        int wrapWidth = GetBool("wordwrap") && HasValue("width") ? GetInt("width") : 0;
        string key = $"{Text}|{GetString("fontface")}|{GetNumber("fontsize")}|{style}|{Color()}|{wrapWidth}";
        if (_cache != null && key == _cacheKey) return _cache;
        var face = new FontFamily(GetString("fontface").Length > 0 ? GetString("fontface") + ", Tahoma, Segoe UI" : "Tahoma");
        var typeface = new Typeface(face, style.Contains("italic") ? FontStyles.Italic : FontStyles.Normal,
            style.Contains("bold") ? FontWeights.Bold : FontWeights.Normal, FontStretches.Normal);
        double px = PointsToPixels(GetNumber("fontsize"));
        var ft = new FormattedText(Text, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight, typeface, px,
            new SolidColorBrush(Color()), VisualTreeHelper.GetDpi(View.Surface).PixelsPerDip) { LineHeight = LineHeight(px) };
        if (style.Contains("underline")) ft.SetTextDecorations(TextDecorations.Underline);
        if (wrapWidth > 0) ft.MaxTextWidth = wrapWidth;
        _cache = ft;
        _cacheKey = key;
        return ft;
    }

    Color Color()
    {
        string attr = !Enabled && GetString("disabledforegroundcolor").Length > 0 ? "disabledforegroundcolor"
            : IsHovered && GetString("hoverforegroundcolor").Length > 0 ? "hoverforegroundcolor" : "foregroundcolor";
        int c = SkinImages.ParseColor(GetString(attr)) ?? 0xFFFFFF;
        return System.Windows.Media.Color.FromRgb((byte)(c >> 16), (byte)(c >> 8), (byte)c);
    }

    public override int NaturalWidth => (int)Math.Ceiling(Measure().WidthIncludingTrailingWhitespace) + 1;
    // Whole lines, one even for no text.
    public override int NaturalHeight => (int)Math.Max(Math.Ceiling(Measure().Height), LineHeight(PointsToPixels(GetNumber("fontsize"))));

    public bool Overflows => !GetBool("wordwrap") && Measure().WidthIncludingTrailingWhitespace > Width + 0.5;
    public bool IsScrolling => GetBool("scrolling") && Overflows && IsEffectivelyVisible;

    protected override void OnPropertyChanged(string name)
    {
        if (name == "value") _scroll = 0;
        if (name is "scrolling" or "value" or "visible" or "width" or "scrollingdelay") View.UpdateScrolling(this);
    }

    /// <summary>Advances the marquee by one step (called from the view's scroll timer).</summary>
    public void ScrollStep()
    {
        double text = Measure().WidthIncludingTrailingWhitespace + 24;
        double amount = Math.Max(1, GetInt("scrollingamount"));
        _scroll = (_scroll + amount) % Math.Max(1, text);
        View.InvalidateContent();
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        string bgAttr = IsHovered && GetString("hoverbackgroundcolor").Length > 0 ? "hoverbackgroundcolor" : "backgroundcolor";
        int? bg = SkinImages.ParseColor(GetString(bgAttr));
        if (bg != null) dc.DrawRectangle(new SolidColorBrush(System.Windows.Media.Color.FromRgb((byte)(bg >> 16), (byte)(bg >> 8), (byte)bg)), null, b);
        if (Text.Length == 0) return;

        var ft = Measure();
        dc.PushClip(new RectangleGeometry(b));
        double y = b.Y + Math.Max(0, (b.Height - ft.Height) / 2);
        if (GetBool("wordwrap")) y = b.Y;
        if (IsScrolling)
        {
            double span = ft.WidthIncludingTrailingWhitespace + 24;
            bool right = GetString("scrollingdirection").Equals("right", StringComparison.OrdinalIgnoreCase);
            double x = right ? b.X - span + _scroll : b.X - _scroll;
            dc.DrawText(ft, new Point(x, y));
            dc.DrawText(ft, new Point(x + span, y));
            if (right) dc.DrawText(ft, new Point(x + 2 * span, y));
        }
        else
        {
            double x = b.X;
            if (GetBool("wordwrap"))
            {
                ft.TextAlignment = GetString("justification").ToLowerInvariant() switch
                {
                    "center" => TextAlignment.Center, "right" => TextAlignment.Right, _ => TextAlignment.Left,
                };
            }
            else
            {
                double w = ft.WidthIncludingTrailingWhitespace;
                string j = GetString("justification").ToLowerInvariant();
                if (j == "center") x = b.X + (b.Width - w) / 2;
                else if (j == "right") x = b.Right - w;
                if (GetBool("truncate") && w > b.Width) ft.Trimming = TextTrimming.CharacterEllipsis;
                if (GetBool("truncate") && w > b.Width) { ft.MaxTextWidth = Math.Max(1, b.Width); ft.MaxLineCount = 1; x = b.X; }
            }
            dc.DrawText(ft, new Point(x, y));
        }
        dc.Pop();
    }

    public override bool IsInteractive => HasHandler("onclick") || HasHandler("onmousedown") || HasHandler("ondblclick") || HasHandler("onmouseup");

    /// <summary>Text that does nothing with the mouse lets clicks through: Blue Crush's title box overlaps its close button.</summary>
    public override bool HitTest(Point local) => base.HitTest(local) && UsesMouse;
}
