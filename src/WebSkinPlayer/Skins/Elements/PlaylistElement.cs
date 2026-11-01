using System.Globalization;
using System.Windows;
using System.Windows.Input;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>
/// PLAYLIST / ITEMSPLAYLIST / DROPDOWNPLAYLIST: the tracks heard this session. WMP drew it as Windows controls,
/// a combo box naming the playlist over a details list in Segoe UI 9 pt, whatever the skin's font. The sizes,
/// colours and column rules were measured in real WMP on a GitHub runner (the playlists, playlist-columns and
/// playlist-widths probes on the Mac branch).
/// </summary>
public sealed class PlaylistElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["backgroundcolor"] = PType.String, ["foregroundcolor"] = PType.String, ["itemplayingcolor"] = PType.String,
        ["itemplayingbackgroundcolor"] = PType.String, ["disableditemcolor"] = PType.String,
        ["backgroundimage"] = PType.String, ["columns"] = PType.String, ["columnsvisible"] = PType.Bool,
        ["fontface"] = PType.String, ["fontsize"] = PType.Number, ["dropdownvisible"] = PType.Bool,
        ["toolbarvisible"] = PType.Bool, ["playlistitemsvisible"] = PType.Bool, ["selectedindex"] = PType.Int,
    };
    protected override Dictionary<string, PType>? Schema => S;

    const double Combo = 24, Toolbar = 29, Header = 24, Row = 19, ScrollBar = 17;
    /// <summary>The space a cell keeps around its text, a header label's wider margin, and the playing band's left gap.</summary>
    const double Pad = 6, HeaderPad = 8, BandInset = 4;
    /// <summary>WMP's columns when a skin names none.</summary>
    const string DefaultColumns = "name=Title;duration=Length;artist=Artist;genre=Genre";

    /// <summary>The first row shown after the wheel scrolled back; null keeps the newest track in view.</summary>
    int? _top;

    public PlaylistElement(SkinView view, WmsNode node) : base(view, node)
    {
        Method("setColumnResizeMode", _ => null);
        Method("setColumnWidth", _ => null);
        Method("getColumnWidth", _ => 0.0);
        Method("sortColumn", _ => null);
        Method("getNextSelectedItem", _ => -1.0);
        Method("getNextCheckedItem", _ => -1.0);
        Method("setCheckedState", _ => null);
        Method("getCheckedState", _ => false);
        Method("deleteSelected", _ => null);
        Method("moveSelectedUp", _ => null);
        Method("moveSelectedDown", _ => null);
        Computed("itemCount", () => (double)Items.Count);
        Computed("playlist", () => View.Theme.Objects.CurrentPlaylist);
    }

    // As WMP drew a PLAYLIST on Windows 11 (measured on a GitHub runner).
    protected override object? ElementDefault(string name) => name.ToLowerInvariant() switch
    {
        "backgroundcolor" => "#FFFFFF", "foregroundcolor" => "#000000", "itemplayingcolor" => "#00FF00",
        "columnsvisible" or "dropdownvisible" or "playlistitemsvisible" => true,
        _ => null,
    };

    public override bool IsInteractive => true;

    List<TrackInfo> Items => View.Theme.Objects.History;

    /// <summary>With its items hidden and no height given, WMP sized a PLAYLIST to its combo and toolbar.</summary>
    public override int NaturalHeight => GetBool("playlistitemsvisible") ? 0
        : (GetBool("dropdownvisible") ? (int)Combo : 0) + (GetBool("toolbarvisible") ? (int)Toolbar : 0);

    sealed record Column(string Field, string Label, double Width);

    sealed record Layout(Rect? Combo, Rect? Toolbar, Rect? Header, Rect Rows, List<Column> Columns, int First, int LastFirst, Rect? VScroll, Rect? HScroll);

    static readonly Typeface ListFace = new("Segoe UI");
    /// <summary>Segoe UI 9 pt in whole pixels, whatever the skin's fontFace and fontSize.</summary>
    static readonly double ListPx = PointsToPixels(9);

    FormattedText Text(string s, int rgb) => new(s, CultureInfo.CurrentUICulture, FlowDirection.LeftToRight, ListFace, ListPx,
        Brush(rgb), VisualTreeHelper.GetDpi(View.Surface).PixelsPerDip);

    double Measure(string s) => s.Length == 0 ? 0 : Text(s, 0).WidthIncludingTrailingWhitespace;

    /// <summary>A track's text in a column; WMP left fields it doesn't know about empty.</summary>
    static string Cell(TrackInfo t, string field) => field switch
    {
        "name" or "title" => t.Title,
        "artist" or "author" => t.Artist,
        "album" or "wm/albumtitle" => t.Album,
        "duration" when t.Duration > 0 => ListTime(t.Duration),
        _ => "",
    };

    /// <summary>"0:40", "12:05", "1:02:05": a list's lengths have no leading zero.</summary>
    static string ListTime(double seconds)
    {
        var t = TimeSpan.FromSeconds(Math.Round(seconds));
        return t.Hours > 0 ? $"{t.Hours}:{t.Minutes:00}:{t.Seconds:00}" : $"{(int)t.TotalMinutes}:{t.Seconds:00}";
    }

    Layout Lay(Rect b)
    {
        double y = b.Y;
        Rect? Take(bool on, double h)
        {
            if (!on) return null;
            var part = new Rect(b.X, y, b.Width, h);
            y += h;
            return part;
        }
        var combo = Take(GetBool("dropdownvisible"), Combo);
        var toolbar = Take(GetBool("toolbarvisible"), Toolbar);
        bool itemsShown = GetBool("playlistitemsvisible");
        var header = Take(itemsShown && GetBool("columnsvisible"), Header);
        double listTop = header?.Y ?? y;
        var items = Items;
        (int First, int LastFirst) Window(double h)
        {
            int lastFirst = Math.Max(0, items.Count - (int)Math.Floor(h / Row));
            return (Math.Min(_top ?? lastFirst, lastFirst), lastFirst);
        }
        double rowsH = itemsShown ? Math.Max(0, b.Bottom - y) : 0;
        int first = Window(rowsH).First, fits = (int)Math.Floor(rowsH / Row);
        string spec = GetString("columns");
        // A Windows list sizes each column to its header and the rows in view, not to the box.
        var columns = (spec.Trim().Length == 0 ? DefaultColumns : spec).Split(';')
            .Select(part => part.Split('=', 2)).Where(kv => kv.Length == 2)
            .Select(kv =>
            {
                string field = kv[0].Trim().ToLowerInvariant(), label = kv[1].Trim();
                double width = items.Skip(first).Take(fits).Select(t => Measure(Cell(t, field)) + 2 * Pad)
                    .Aggregate(Measure(label) + 2 * HeaderPad, Math.Max);
                return new Column(field, label, Math.Round(width));
            }).ToList();
        double total = columns.Sum(c => c.Width);
        bool vertical = itemsShown && items.Count * Row > rowsH;
        double room = b.Width - (vertical ? ScrollBar : 0);
        Rect? hscroll = itemsShown && total > room ? new Rect(b.X, b.Bottom - ScrollBar, room, ScrollBar) : null;
        if (hscroll != null) rowsH = Math.Max(0, rowsH - ScrollBar);
        var rows = new Rect(b.X, y, b.Width, rowsH);
        var (shownFirst, last) = Window(rowsH);
        Rect? vscroll = vertical ? new Rect(b.Right - ScrollBar, listTop, ScrollBar, Math.Max(0, rows.Bottom - listTop)) : null;
        return new Layout(combo, toolbar, header, rows, columns, shownFirst, last, vscroll, hscroll);
    }

    /// <summary>One row per wheel step. Scrolling back to the end follows the newest track again.</summary>
    public override void OnMouseWheel(int delta)
    {
        var l = Lay(new Rect(0, 0, Width, Height));
        int first = delta > 0 ? Math.Max(0, l.First - 1) : l.First + 1;
        _top = first < l.LastFirst ? first : null;
        View.InvalidateVisual();
    }

    public override void OnMouseDown(Point local, MouseButton button)
    {
        base.OnMouseDown(local, button);
        View.Focus(this);
    }

    static SolidColorBrush Brush(int rgb) => new(Color.FromRgb((byte)(rgb >> 16), (byte)(rgb >> 8), (byte)rgb));

    static void Fill(DrawingContext dc, Rect r, int rgb)
    {
        if (r.Width > 0 && r.Height > 0) dc.DrawRectangle(Brush(rgb), null, r);
    }

    int Colour(string attr, int fallback) => SkinImages.ParseColor(GetString(attr)) ?? fallback;

    /// <summary>The longest start of <paramref name="s"/> that fits <paramref name="room"/> with "..." after it, as a Windows list cut it.</summary>
    string Ellipsize(string s, double room)
    {
        if (Measure(s) <= room) return s;
        int lo = 0, hi = s.Length;
        while (lo < hi)
        {
            int mid = (lo + hi + 1) / 2;
            if (Measure(s[..mid] + "...") <= room) lo = mid; else hi = mid - 1;
        }
        return s[..lo] + "...";
    }

    /// <summary>The width a cell's text may take; the column was rounded from the same measure, so allow that rounding.</summary>
    static double TextRoom(double width) => Math.Max(1, width - 2 * Pad + 0.5);

    double TextX(string field, string s, double x, double width) => field == "duration" ? x + width - Pad - Measure(s) : x + Pad;

    void Draw(DrawingContext dc, string s, double x, double y, int rgb, Rect clip)
    {
        if (s.Length == 0 || clip.IsEmpty || clip.Width <= 0 || clip.Height <= 0) return;
        dc.PushClip(new RectangleGeometry(clip));
        dc.DrawText(Text(s, rgb), new Point(x, y));
        dc.Pop();
    }

    public override void Render(DrawingContext dc, Rect b)
    {
        if (b.Width < 4 || b.Height < 4) return;
        dc.PushClip(new RectangleGeometry(b));
        var l = Lay(b);
        Fill(dc, b, Colour("backgroundcolor", 0xFFFFFF));
        if (Img("backgroundimage") is { } bg) dc.DrawImage(bg.Source, new Rect(b.X, b.Y, bg.Width, bg.Height));
        string name = ToStr(View.Theme.Objects.CurrentPlaylist.Get("name"));
        if (l.Combo is { } combo) DrawCombo(dc, combo, name);
        if (l.Toolbar is { } toolbar) DrawToolbar(dc, toolbar, name);
        if (l.Header is { } h)
        {
            Fill(dc, h, 0xFFFFFF);
            double x = h.X;
            foreach (var col in l.Columns)
            {
                string label = Ellipsize(col.Label, TextRoom(col.Width));
                Draw(dc, label, TextX(col.Field, label, x, col.Width), h.Y + 4, 0, Rect.Intersect(new Rect(x, h.Y, col.Width, h.Height), h));
                Fill(dc, new Rect(x + col.Width - 1, h.Y, 1, h.Height), 0xE5E5E5);
                x += col.Width;
            }
        }
        var rows = l.Rows;
        double total = l.Columns.Sum(c => c.Width);
        int fg = Colour("foregroundcolor", 0), playing = Colour("itemplayingcolor", 0x00FF00), band = Colour("itemplayingbackgroundcolor", 0x222222);
        var items = Items;
        if (rows.Height > 0)
        {
            dc.PushClip(new RectangleGeometry(rows));
            for (int i = l.First; i < items.Count; i++)
            {
                double y = rows.Y + (i - l.First) * Row;
                if (y >= rows.Bottom) break;
                bool current = i == items.Count - 1;
                if (current) Fill(dc, new Rect(rows.X + BandInset, y, Math.Max(0, total - BandInset), Row), band);
                double x = rows.X;
                foreach (var col in l.Columns)
                {
                    string s = Cell(items[i], col.Field);
                    if (s.Length > 0)
                    {
                        s = Ellipsize(s, TextRoom(col.Width));
                        Draw(dc, s, TextX(col.Field, s, x, col.Width), y + 2, current ? playing : fg, Rect.Intersect(new Rect(x, y, col.Width, Row), rows));
                    }
                    x += col.Width;
                }
            }
            if (items.Count == 0 && rows.Height >= Row)
                Draw(dc, Ellipsize("Play something in your browser", Math.Max(1, rows.Width - 2 * Pad)), rows.X + Pad, rows.Y + 2, fg, rows);
            dc.Pop();
        }
        if (l.VScroll is { } vs) DrawScrollBar(dc, vs, vertical: true, l.First, rows.Height / Row, items.Count);
        if (l.HScroll is { } hs) DrawScrollBar(dc, hs, vertical: false, 0, hs.Width, total);
        if (l.VScroll is { } v2 && l.HScroll is { } h2) Fill(dc, new Rect(v2.X, h2.Y, v2.Width, h2.Height), 0xF0F0F0);
        dc.Pop();
    }

    /// <summary>A Windows 11 combo box: a one-pixel frame with a darker bottom edge, the playlist icon, the name and a chevron.</summary>
    void DrawCombo(DrawingContext dc, Rect r, string name)
    {
        dc.PushClip(new RectangleGeometry(r));
        Fill(dc, r, 0xD2D2D2);
        Fill(dc, new Rect(r.X, r.Bottom - 1, r.Width, 1), 0xBCBCBC);
        foreach (var (x, y) in new[] { (r.X, r.Y), (r.Right - 1, r.Y), (r.X, r.Bottom - 1), (r.Right - 1, r.Bottom - 1) })
            Fill(dc, new Rect(x, y, 1, 1), 0xE6E6E6);
        Fill(dc, new Rect(r.X + 1, r.Y + 1, r.Width - 2, r.Height - 2), 0xFDFDFD);
        // The page with a play triangle that WMP put before a playlist's name.
        double ix = r.X + 6, iy = r.Y + 4;
        Fill(dc, new Rect(ix, iy, 11, 13), 0x758899);
        Fill(dc, new Rect(ix + 1, iy + 1, 9, 11), 0xEEF2F8);
        foreach (var (row, len) in new[] { (4.0, 2.0), (5.0, 3.0), (6.0, 4.0), (7.0, 3.0), (8.0, 2.0) })
            Fill(dc, new Rect(ix + 3, iy + row, len, 1), 0x6E8BA5);
        Draw(dc, name, r.X + 24, r.Y + 5, 0, new Rect(r.X + 24, r.Y, Math.Max(0, r.Width - 24 - 17), r.Height));
        double cx = r.Right - 13, cy = r.Y + 10;
        for (int k = 0; k < 4; k++)
        {
            Fill(dc, new Rect(cx + k, cy + k, 1, 1), 0x6E6E6E);
            Fill(dc, new Rect(cx + 7 - k, cy + k, 1, 1), 0x6E6E6E);
        }
        dc.Pop();
    }

    /// <summary>The light blue strip under the combo with the playlist's name and a menu arrow.</summary>
    void DrawToolbar(DrawingContext dc, Rect r, string name)
    {
        dc.PushClip(new RectangleGeometry(r));
        Fill(dc, new Rect(r.X, r.Y, r.Width, r.Height - 3), 0xDFEAF7);
        Fill(dc, new Rect(r.X, r.Bottom - 3, r.Width, 2), 0xDCDFE0);
        Fill(dc, new Rect(r.X, r.Bottom - 1, r.Width, 1), 0xA9B7C9);
        Draw(dc, name, r.X + 9, r.Y + 6, 0, r);
        double x = r.X + 9 + Measure(name) + 8;
        for (int k = 0; k < 4; k++) Fill(dc, new Rect(x + k, r.Y + 12 + k, 7 - 2 * k, 1), 0);
        dc.Pop();
    }

    /// <summary>Windows 11's thin scroll bar at rest: a light track with a two-pixel thumb line between the arrow buttons.</summary>
    static void DrawScrollBar(DrawingContext dc, Rect r, bool vertical, double at, double shown, double total)
    {
        Fill(dc, r, 0xF0F0F0);
        double span = (vertical ? r.Height : r.Width) - 2 * ScrollBar;
        if (span <= 0 || total <= 0) return;
        double len = Math.Max(8, span * Math.Min(1, shown / total));
        double start = ScrollBar + (span - len) * Math.Min(1, at / Math.Max(1, total - shown));
        Fill(dc, vertical ? new Rect(r.X + 7, r.Y + start, 2, len) : new Rect(r.X + start, r.Y + 7, len, 2), 0x858585);
    }
}
