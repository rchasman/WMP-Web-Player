using System.Globalization;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>
/// LISTBOX: WMP's docs make it a POPUP shown in place. The Skins Factory's playlist choosers fill it from
/// script and act on selectedItem_onChange. Drawn as WMP drew it on Windows 11 (measured on a runner): rows
/// 1.3 times the font's pixel size, text 2 pixels in, a one-pixel #7A7A7A border, the selected row in the
/// Windows highlight.
/// </summary>
public sealed class ListBoxElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["selecteditem"] = PType.Int, ["firstvisibleitem"] = PType.Int, ["backgroundcolor"] = PType.String,
        ["foregroundcolor"] = PType.String, ["fontface"] = PType.String, ["fontsize"] = PType.Number,
        ["fontstyle"] = PType.String, ["border"] = PType.Bool, ["sorted"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    const int Highlight = 0x0078D4, BorderColor = 0x7A7A7A;

    readonly List<string> _items;

    public ListBoxElement(SkinView view, WmsNode node) : base(view, node)
    {
        _items = ItemLines(node);
        Method("appendItem", a => Append(ToStr(Arg(a, 0))));
        Method("insertItem", a =>
        {
            _items.Insert(Math.Clamp((int)ToNumber(Arg(a, 0)), 0, _items.Count), ToStr(Arg(a, 1)));
            View.InvalidateVisual();
            return null;
        });
        Method("deleteItem", a => { Delete((int)ToNumber(Arg(a, 0))); return null; });
        Method("deleteAll", _ =>
        {
            _items.Clear();
            SetValue("selecteditem", -1.0, notify: false);
            SetValue("firstvisibleitem", 0.0, notify: false);
            View.InvalidateVisual();
            return null;
        });
        Method("getItem", a => Item((int)ToNumber(Arg(a, 0))) ?? "");
        Method("replaceItem", a =>
        {
            int i = (int)ToNumber(Arg(a, 0));
            if (Item(i) != null) { _items[i] = ToStr(Arg(a, 1)); View.InvalidateVisual(); }
            return null;
        });
        Method("findItem", a => (double)Find((int)ToNumber(Arg(a, 0)), ToStr(Arg(a, 1))));
        Method("getNextSelectedItem", a => GetInt("selecteditem") > (int)ToNumber(Arg(a, 0)) ? (double)GetInt("selecteditem") : -1.0);
        Method("setSelectedState", a =>
        {
            int i = (int)ToNumber(Arg(a, 0));
            if (ToBool(Arg(a, 1))) SetValue("selecteditem", (double)i);
            else if (GetInt("selecteditem") == i) SetValue("selecteditem", -1.0);
            return null;
        });
        Method("show", _ => { SetValue("visible", true); return null; });
        Method("dismiss", _ => { SetValue("visible", false); return null; });
        Computed("itemCount", () => (double)_items.Count);
    }

    /// <summary>The values of a POPUP's or LISTBOX's ITEM children, which come before lines added by script.</summary>
    public static List<string> ItemLines(WmsNode node) =>
        node.Children.Where(c => c.Tag == "ITEM").Select(c => c.Attr("value") ?? "").ToList();

    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "selecteditem" => -1.0,
        "backgroundcolor" => "#FFFFFF",
        "foregroundcolor" => "#000000",
        "fontface" => "Segoe UI",
        "fontsize" => 9.0,
        "fontstyle" => "normal",
        "border" => true,
        _ => null,
    };

    public override bool IsInteractive => true;

    string? Item(int i) => i >= 0 && i < _items.Count ? _items[i] : null;

    object Append(string line)
    {
        int at = GetBool("sorted")
            ? _items.TakeWhile(i => string.Compare(i, line, StringComparison.OrdinalIgnoreCase) <= 0).Count()
            : _items.Count;
        _items.Insert(at, line);
        View.InvalidateVisual();
        return (double)at;
    }

    void Delete(int i)
    {
        if (Item(i) == null) return;
        _items.RemoveAt(i);
        int selected = GetInt("selecteditem");
        if (selected >= i) SetValue("selecteditem", selected == i ? -1.0 : selected - 1.0, notify: false);
        View.InvalidateVisual();
    }

    /// <summary>A Windows list box's find: the first line after <paramref name="start"/> that begins with the text, ignoring case, wrapping round.</summary>
    int Find(int start, string text)
    {
        int n = _items.Count;
        int from = Math.Max(0, start + 1);
        return Enumerable.Range(0, n).Select(k => (from + k) % n)
            .Where(i => _items[i].StartsWith(text, StringComparison.OrdinalIgnoreCase)).DefaultIfEmpty(-1).First();
    }

    /// <summary>Row height: 1.3 times the font's pixel size, rounded up (Arial 8 pt gives 14, Segoe UI 9 pt 16).</summary>
    double Row => Math.Ceiling(Math.Max(6, GetNumber("fontsize")) * 96.0 / 72.0 * 1.3);

    double Inset => GetBool("border") ? 1 : 0;

    /// <summary>The line under a local point, or -1.</summary>
    public int ItemAt(Point local)
    {
        int i = Math.Max(0, GetInt("firstvisibleitem")) + (int)Math.Floor(Math.Max(0, local.Y - Inset) / Row);
        return i < _items.Count ? i : -1;
    }

    public override void OnMouseDown(Point local, MouseButton button)
    {
        base.OnMouseDown(local, button);
        if (ItemAt(local) is var i and >= 0) SetValue("selecteditem", (double)i);
    }

    public override void OnMouseWheel(int delta)
    {
        int shown = Math.Max(1, (int)Math.Floor((Height - 2 * Inset) / Row));
        int last = Math.Max(0, _items.Count - shown);
        SetValue("firstvisibleitem", (double)Math.Clamp(GetInt("firstvisibleitem") - Math.Sign(delta), 0, last));
    }

    static SolidColorBrush Brush(int rgb) => new(Color.FromRgb((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb));

    public override void Render(DrawingContext dc, Rect b)
    {
        dc.PushClip(new RectangleGeometry(b));
        double inset = Inset;
        if (inset > 0) dc.DrawRectangle(Brush(BorderColor), null, b);
        var inner = new Rect(b.X + inset, b.Y + inset, Math.Max(0, b.Width - 2 * inset), Math.Max(0, b.Height - 2 * inset));
        dc.PushClip(new RectangleGeometry(inner));
        if (SkinImages.ParseColor(GetString("backgroundcolor")) is int bg) dc.DrawRectangle(Brush(bg), null, inner);
        int fg = SkinImages.ParseColor(GetString("foregroundcolor")) ?? 0;
        string style = GetString("fontstyle").ToLowerInvariant();
        var typeface = new Typeface(new FontFamily(GetString("fontface") + ", Segoe UI"), style.Contains("italic") ? FontStyles.Italic : FontStyles.Normal,
            style.Contains("bold") ? FontWeights.Bold : FontWeights.Normal, FontStretches.Normal);
        double px = PointsToPixels(Math.Max(6, GetNumber("fontsize"))), row = Row, dip = VisualTreeHelper.GetDpi(View.Surface).PixelsPerDip;
        int selected = GetInt("selecteditem");
        double y = inner.Y;
        for (int i = Math.Max(0, GetInt("firstvisibleitem")); i < _items.Count && y < inner.Bottom; i++, y += row)
        {
            if (i == selected) dc.DrawRectangle(Brush(Highlight), null, new Rect(inner.X, y, inner.Width, row));
            var ft = new FormattedText(_items[i], CultureInfo.CurrentUICulture, FlowDirection.LeftToRight, typeface, px,
                Brush(i == selected ? 0xFFFFFF : fg), dip)
            { MaxTextWidth = Math.Max(1, inner.Width - 4), MaxLineCount = 1, Trimming = TextTrimming.CharacterEllipsis };
            dc.DrawText(ft, new Point(inner.X + 2, y + 1));
        }
        dc.Pop();
        dc.Pop();
    }
}
