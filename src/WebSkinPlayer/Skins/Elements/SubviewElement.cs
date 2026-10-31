using System.Windows;
using System.Windows.Media;
using WebSkinPlayer.Skins.Scripting;

namespace WebSkinPlayer.Skins.Elements;

/// <summary>SUBVIEW: a container with an optional background colour/image and clipping shape.</summary>
public class SubviewElement : SkinElement
{
    static readonly Dictionary<string, PType> S = new(StringComparer.OrdinalIgnoreCase)
    {
        ["backgroundcolor"] = PType.String, ["backgroundimage"] = PType.String, ["backgroundtiled"] = PType.Bool,
        ["resizebackgroundimage"] = PType.Bool, ["resizeimages"] = PType.Bool,
    };
    protected override Dictionary<string, PType>? Schema => S;

    public SubviewElement(SkinView? view, WmsNode node) : base(view, node) { }

    public override bool ClipsChildren => true;

    // A VIEW keeps its clippingColor pixels here: they are keyed out of the composed window
    // (so backgroundColor shows only through transparencyColor holes, as in WMP).
    SkinBitmap? Background =>
        View.Theme.Images.Get(GetString("backgroundimage"),
            SkinImages.ParseColor(GetString("transparencycolor")),
            GetNumber("hueshift"), GetNumber("saturation"),
            GetString("clippingimage").Length == 0 && (this is not SkinView || GetBool("titlebar")) ? SkinImages.ParseColor(GetString("clippingcolor")) : null);

    // With no picture a subview holds its children, as WMP showed them: Catwoman's time digits sit in
    // a subview given only a position. A text sized by its words adds nothing, so the string tables
    // many skins keep in such a subview (WoW, Disney) stay hidden. A VIEW with no picture has no size
    // of its own.
    public override int NaturalWidth => Background?.Width ?? (this is SkinView ? 0 : SizingChildren.Select(c => c.Left + c.Width).Append(0).Max());
    public override int NaturalHeight => Background?.Height ?? (this is SkinView ? 0 : SizingChildren.Select(c => c.Top + c.Height).Append(0).Max());

    IEnumerable<SkinElement> SizingChildren => Children.Where(c => c is not TextElement || c.HasValue("width"));

    protected override string PrimaryImage => GetString("backgroundimage");
    public override bool MaskTiled => GetBool("backgroundtiled") && GetString("clippingimage").Length == 0;

    /// <summary>
    /// Hit where it draws: its colour, or its background's pixel there in the frame shown now (anime's
    /// animated background, 9 Series' see-through tiles). One with nothing to draw lets clicks pass
    /// unless it uses the mouse: Constantine and Media Center group controls in window-sized empty
    /// subviews above other controls. The VIEW itself always takes them (to drag the window).
    /// </summary>
    public override bool HitTest(Point local)
    {
        if (!base.HitTest(local) || !MaskAllows(local)) return false;
        if (BackgroundColor != null) return true;
        var bg = Background;
        if (bg == null) return this is SkinView || UsesMouse;
        return BackgroundAlpha(bg, local) > 0;
    }

    public override bool HitTestImage(Point local)
    {
        if (!base.HitTest(local) || !MaskAllows(local)) return false;
        return Background is { } bg && BackgroundAlpha(bg, local) > 0;
    }

    int BackgroundAlpha(SkinBitmap bg, Point local)
    {
        var (x, y) = PixelLookup(bg)(local);
        return bg.Alpha(ShownFrame(bg, GetString("backgroundimage")), x, y);
    }

    /// <summary>Maps a point of this subview to the pixel of <paramref name="bg"/> drawn there: stretched, tiled or once at its top-left.</summary>
    Func<Point, (int X, int Y)> PixelLookup(SkinBitmap bg)
    {
        if (Stretched && Width > 0 && Height > 0)
        {
            double sx = (double)bg.Width / Width, sy = (double)bg.Height / Height;
            return p => ((int)Math.Floor(p.X * sx), (int)Math.Floor(p.Y * sy));
        }
        if (GetBool("backgroundtiled"))
            return p => (((int)Math.Floor(p.X) % bg.Width + bg.Width) % bg.Width, ((int)Math.Floor(p.Y) % bg.Height + bg.Height) % bg.Height);
        return p => ((int)Math.Floor(p.X), (int)Math.Floor(p.Y));
    }

    public int? BackgroundColor
    {
        get
        {
            int? color = SkinImages.ParseColor(GetString("backgroundcolor"));
            if (color == null && Values.TryGetValue("backgoundcolor", out var typo)) color = SkinImages.ParseColor(ToStr(typo));
            return color;
        }
    }

    public override void RenderBackdrop(DrawingContext dc, Rect b)
    {
        if (BackgroundColor is int color)
            dc.DrawRectangle(new SolidColorBrush(Color.FromRgb((byte)(color >> 16), (byte)(color >> 8), (byte)color)), null, b);
    }

    bool Stretched => GetBool("resizebackgroundimage") || GetBool("resizeimages") || SkinImages.IsDynamic(GetString("backgroundimage"));

    public override void Render(DrawingContext dc, Rect b)
    {
        var bg = Background;
        if (bg == null) return;
        var src = FrameOf(bg, GetString("backgroundimage"));
        if (Stretched)
            dc.DrawImage(src, b);
        else if (GetBool("backgroundtiled"))
        {
            for (double y = b.Top; y < b.Bottom; y += bg.Height)
                for (double x = b.Left; x < b.Right; x += bg.Width)
                    dc.DrawImage(src, new Rect(x, y, bg.Width, bg.Height));
        }
        else
            dc.DrawImage(src, new Rect(b.X, b.Y, bg.Width, bg.Height));
    }

    /// <summary>
    /// The colour that leaves this subview's background open (its transparencyColor), or null. WMP's dynamic
    /// images (album art) are not the skin's own pixels, so they never match its key.
    /// </summary>
    public int? HoleKey => SkinImages.IsDynamic(GetString("backgroundimage")) ? null : SkinImages.ParseColor(GetString("transparencycolor"));

    /// <summary>
    /// What the background image shows at each point of this subview, looked up as <see cref="Render"/>
    /// draws it (stretched, tiled or plain), or null when it has no image.
    /// </summary>
    public Func<Point, Backdrop>? BackdropMap()
    {
        var bg = Background;
        if (bg == null || Width <= 0 || Height <= 0) return null;
        int? key = HoleKey;
        var pixel = PixelLookup(bg);
        return local =>
        {
            var (x, y) = pixel(local);
            if (key is int k && SkinImages.MatchesKey(bg.Rgb(x, y), k)) return Backdrop.Open;
            return bg.Alpha(x, y) > 0 ? Backdrop.Covered : Backdrop.Outside;
        };
    }
}

/// <summary>A subview's background at one pixel.</summary>
public enum Backdrop
{
    /// <summary>Its transparencyColor: see-through.</summary>
    Open,
    /// <summary>A visible pixel.</summary>
    Covered,
    Outside,
}
