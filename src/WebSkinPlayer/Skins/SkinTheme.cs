using System.IO;
using System.Media;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Visualization;

namespace WebSkinPlayer.Skins;

/// <summary>A loaded skin (.wmz/.wms): package, images, object model and its open views.</summary>
public sealed class SkinTheme
{
    public string Path { get; }
    public SkinPackage Package { get; }
    public SkinImages Images { get; }
    public WmpObjects Objects { get; }
    public string Title { get; }
    public string Author { get; }
    public string Copyright { get; }
    public bool HasEffects { get; }
    public SkinView? ActiveView { get; set; }
    public IReadOnlyList<SkinView> OpenViews => _open;
    public event Action? Closed;

    readonly WmsNode _theme;
    readonly List<WmsNode> _viewNodes;
    readonly List<SkinView> _open = new();
    readonly bool _preview;
    bool _closed;

    /// <summary>The skin's settings key (its file name): preferences and its remembered visualization.</summary>
    public string Key => KeyFor(Path);
    public static string KeyFor(string path) => System.IO.Path.GetFileName(path).ToLowerInvariant();
    string PrefKey => Key;

    SkinTheme(string path, bool preview)
    {
        Path = System.IO.Path.GetFullPath(path);
        _preview = preview;
        Package = SkinPackage.Open(Path);
        Images = new SkinImages(Package);
        var root = WmsParser.Parse(Package.ReadText(Package.DefinitionFile));
        _theme = root.Children.FirstOrDefault(n => n.Tag == "THEME") ?? root;
        _viewNodes = Descendants(_theme).Where(n => n.Tag == "VIEW").ToList();
        if (_viewNodes.Count == 0) throw new InvalidDataException("The skin definition has no VIEW.");
        Title = _theme.Attr("title") ?? System.IO.Path.GetFileNameWithoutExtension(Path);
        Author = _theme.Attr("author") ?? "";
        Copyright = _theme.Attr("copyright") ?? "";
        HasEffects = Descendants(_theme).Any(n => n.Tag is "EFFECTS" or "WMPEFFECTS");
        Objects = new WmpObjects(this, App.Services.Player);
    }

    static IEnumerable<WmsNode> Descendants(WmsNode n)
    {
        foreach (var c in n.Children)
        {
            yield return c;
            foreach (var d in Descendants(c)) yield return d;
        }
    }

    public static SkinTheme Load(string path) => new(path, preview: false);

    /// <summary>The view whose closing ends the skin (the first view, or whatever theme.currentViewID switched to).</summary>
    SkinView? _main;

    public void OpenMainView()
    {
        Objects.Attach();
        _main = OpenNode(_viewNodes[0], null, null);
    }

    /// <summary>theme.currentViewID = "x": replace the current view with another one.</summary>
    public void SwitchToView(string id)
    {
        var from = ActiveView;
        var node = _viewNodes.FirstOrDefault(n => string.Equals(n.Attr("id"), id, StringComparison.OrdinalIgnoreCase));
        if (node == null || _closed || _preview) return;
        if (from != null && string.Equals(from.Id, id, StringComparison.OrdinalIgnoreCase)) return;
        // Deferred: this is usually called from inside the old view's onLoad.
        System.Windows.Threading.Dispatcher.CurrentDispatcher.InvokeAsync(() =>
        {
            if (_closed) return;
            var existing = _open.FirstOrDefault(v => string.Equals(v.Id, id, StringComparison.OrdinalIgnoreCase));
            SkinView next;
            try { next = existing ?? OpenNode(node, null, null); }
            catch (Exception ex) { Log.Write($"currentViewID switch to {id} failed: {ex}"); return; }
            _main = next;
            ActiveView = next;
            if (from != null && from != next && _open.Remove(from)) from.CloseWindow();
        });
    }

    SkinView OpenNode(WmsNode node, double? x, double? y)
    {
        var view = new SkinView(this, node);
        _open.Add(view);
        ActiveView ??= view;
        view.Open(x, y);
        if (view.Bridge.ErrorCount > 0) Log.Write($"{System.IO.Path.GetFileName(Path)}: {view.Bridge.ErrorCount} script error(s) while opening view {view.Id}");
        return view;
    }

    public void OpenView(string id, double? x, double? y)
    {
        var existing = _open.FirstOrDefault(v => string.Equals(v.Id, id, StringComparison.OrdinalIgnoreCase));
        if (existing != null) { existing.Window?.Activate(); return; }
        var node = _viewNodes.FirstOrDefault(n => string.Equals(n.Attr("id"), id, StringComparison.OrdinalIgnoreCase));
        if (node == null) { Log.Write($"openView: no view '{id}'"); return; }
        try { OpenNode(node, x, y); }
        catch (Exception ex) { Log.Write($"openView {id} failed: {ex}"); }
    }

    public void CloseView(string id)
    {
        var v = _open.FirstOrDefault(x => string.Equals(x.Id, id, StringComparison.OrdinalIgnoreCase));
        if (v != null) CloseView(v);
    }

    public void CloseView(SkinView view)
    {
        if (!_open.Remove(view)) return;
        view.CloseWindow();
        if (ActiveView == view) ActiveView = _main != view ? _main : _open.FirstOrDefault();
        // Closing the main view, or the last one, ends skin mode like WMP did.
        if (_open.Count == 0 || view == _main) Close();
    }

    public void Close()
    {
        if (_closed) return;
        _closed = true;
        foreach (var v in _open.ToArray()) { _open.Remove(v); v.CloseWindow(); }
        Objects.Detach();
        App.Services.Settings.Save();
        Closed?.Invoke();
    }

    public void InvalidateAll()
    {
        foreach (var v in _open) v.InvalidateVisual();
    }

    /// <summary>Hide/show every open view (collapsing into the taskbar widget keeps the skin running).</summary>
    public void SetWindowsVisible(bool visible)
    {
        foreach (var v in _open)
        {
            if (v.Window == null) continue;
            if (visible) { v.Window.Show(); if (v.Window.WindowState == WindowState.Minimized) v.Window.WindowState = WindowState.Normal; }
            else v.Window.Hide();
        }
    }

    // ---- theme object services -----------------------------------------------------------

    public string LoadPreference(string name)
    {
        var prefs = App.Services.Settings.SkinPreferences;
        return prefs.TryGetValue(PrefKey, out var d) && d.TryGetValue(name, out var v) ? v : "--";
    }

    /// <summary>Kept in the settings, which a test run never writes to disk: skins read their own back (Disney's intro).</summary>
    public void SavePreference(string name, string value)
    {
        if (_preview) return;
        var prefs = App.Services.Settings.SkinPreferences;
        if (!prefs.TryGetValue(PrefKey, out var d)) prefs[PrefKey] = d = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        d[name] = value;
    }

    public string LoadString(string id)
    {
        if (WmpResources.IsResourceUrl(id)) return WmpResources.ReadText(id) ?? "";
        if (id.StartsWith('#') || int.TryParse(id, out _)) return WmpResources.ReadText("res://wmploc.dll/RT_STRING/" + (id.StartsWith('#') ? id : "#" + id)) ?? "";
        return id;
    }

    public void PlaySound(string file)
    {
        if (Sandbox.Enabled) return;
        try
        {
            var bytes = WmpResources.IsResourceUrl(file) ? WmpResources.ReadBytes(file) : Package.ReadBytes(file);
            if (bytes == null) return;
            new SoundPlayer(new MemoryStream(bytes)).Play();
        }
        catch (Exception ex) { Log.Write("playSound failed: " + ex.Message); }
    }

    public void OpenDialog(string type, string param)
    {
        switch (type.ToUpperInvariant())
        {
            case "FILE_OPEN": case "URL_OPEN": case "OPEN":
                ShowAppMenu(ActiveView, "open", null);
                break;
            case "SKIN_CHOOSER": case "SKINS":
                App.Modes.ShowFullMode();
                break;
            default:
                Log.Write($"theme.openDialog('{type}') is not supported");
                break;
        }
    }

    /// <summary>The right-click / AUTOMENU menu: transport, visualizations, sources and modes.</summary>
    public void ShowAppMenu(SkinView? view, string which, Point? at)
    {
        var pm = App.Services.Player;
        var vis = App.Services.Visualizations;
        var menu = new ContextMenu();
        MenuItem Item(string header, Action a, bool enabled = true, bool? check = null)
        {
            var mi = new MenuItem { Header = header, IsEnabled = enabled };
            if (check.HasValue) { mi.IsCheckable = true; mi.IsChecked = check.Value; }
            mi.Click += (_, _) => a();
            return mi;
        }

        var open = new MenuItem { Header = "Open" };
        open.Items.Add(Item("SoundCloud", () => WmpObjects.OpenInBrowser("https://soundcloud.com/discover")));
        open.Items.Add(Item("Spotify Web Player", () => WmpObjects.OpenInBrowser("https://open.spotify.com/")));
        open.Items.Add(Item("YouTube Music", () => WmpObjects.OpenInBrowser("https://music.youtube.com/")));

        var play = new MenuItem { Header = "Play" };
        play.Items.Add(Item(pm.PlayState == Media.WmpState.psPlaying ? "Pause" : "Play", pm.TogglePlayPause));
        play.Items.Add(Item("Stop", pm.Stop));
        play.Items.Add(Item("Previous", pm.Previous, pm.IsAvailable("previous")));
        play.Items.Add(Item("Next", pm.Next, pm.IsAvailable("next")));
        play.Items.Add(new Separator());
        play.Items.Add(Item("Mute", () => pm.Mute = !pm.Mute, true, pm.Mute));
        play.Items.Add(Item("Volume Up", () => pm.Volume += 5));
        play.Items.Add(Item("Volume Down", () => pm.Volume -= 5));

        var visMenu = new MenuItem { Header = "Visualizations" };
        foreach (var d in VisualizationCatalog.All)
        {
            var sub = new MenuItem { Header = d.Title };
            for (int i = 0; i < d.Presets.Count; i++)
            {
                int preset = i;
                sub.Items.Add(Item(d.Presets[i], () => vis.Choose(d.Id, preset), true, vis.EffectId == d.Id && vis.Preset == i));
            }
            visMenu.Items.Add(sub);
        }

        if (which.Equals("open", StringComparison.OrdinalIgnoreCase)) { foreach (var i in open.Items.Cast<object>().ToList()) { open.Items.Remove(i); menu.Items.Add(i); } }
        else if (which.Equals("play", StringComparison.OrdinalIgnoreCase)) { foreach (var i in play.Items.Cast<object>().ToList()) { play.Items.Remove(i); menu.Items.Add(i); } }
        else
        {
            menu.Items.Add(open);
            menu.Items.Add(play);
            menu.Items.Add(visMenu);
            menu.Items.Add(UI.VisualizationColorMenu.Build(view?.Window));
            menu.Items.Add(new Separator());
            menu.Items.Add(Item("Always on Top", App.Modes.ToggleAlwaysOnTop, true, App.Modes.AlwaysOnTop));
            if (App.ScreenSaver is { } saver) menu.Items.Add(saver.BuildMenu());
            menu.Items.Add(Item("Skin Chooser…", App.Modes.ShowFullMode));
            menu.Items.Add(Item("Switch to Full Mode\tCtrl+1", App.Modes.ShowFullMode));
            menu.Items.Add(Item("Turn into Taskbar Widget\tCtrl+3", App.Modes.CollapseToWidget));
            menu.Items.Add(Item("Keep Widget on the Taskbar", App.Modes.ToggleWidgetAlwaysOn, true, App.Services.Settings.WidgetAlwaysOn));
            if (UpdateChecker.Available is { } update)
            {
                var updateMenu = new MenuItem { Header = $"Update Available (v{update.Version})" };
                updateMenu.Items.Add(Item("Install Now…", () => UI.UpdateWindow.Start(update, view?.Window)));
                updateMenu.Items.Add(Item("What's New…", () => WmpObjects.OpenInBrowser(update.PageUrl)));
                menu.Items.Add(updateMenu);
            }
            menu.Items.Add(new Separator());
            menu.Items.Add(Item("Exit", App.Modes.Exit));
        }
        if (view?.Surface != null)
        {
            menu.PlacementTarget = view.Surface;
            if (at.HasValue)
            {
                menu.Placement = System.Windows.Controls.Primitives.PlacementMode.Relative;
                menu.HorizontalOffset = at.Value.X;
                menu.VerticalOffset = at.Value.Y;
            }
        }
        menu.IsOpen = true;
    }

    // ---- previews ------------------------------------------------------------------------

    /// <summary>Renders a skin's main view offscreen (scripts and onLoad run, nothing is shown).</summary>
    public static BitmapSource? RenderPreview(string path) => RenderPreview(path, null);

    /// <param name="chooserMode">True (as WMP's Skin Chooser): scripts run without a player object.
    /// False: scripts see the player, as when the skin is applied (used to count script errors).</param>
    public static BitmapSource? RenderPreview(string path, List<string>? errors, System.Text.StringBuilder? tree = null, bool chooserMode = true)
    {
        var theme = new SkinTheme(path, preview: true);
        var view = new SkinView(theme, theme._viewNodes[0], forPreview: chooserMode, errors);
        view.LoadForPreview();
        if (tree != null) DumpTree(view, view, 0, tree);
        int w = Math.Clamp(view.Width, 1, 2000), h = Math.Clamp(view.Height, 1, 2000);
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen()) view.RenderTree(dc);
        var rtb = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        foreach (var el in view.AllElements) el.OnUnloaded();
        return TrimTransparent(rtb);
    }

    /// <summary>Crops fully transparent margins (many skins sit in a much larger transparent view).</summary>
    static BitmapSource TrimTransparent(BitmapSource src)
    {
        int w = src.PixelWidth, h = src.PixelHeight;
        var px = new int[w * h];
        src.CopyPixels(px, w * 4, 0);
        int minX = w, minY = h, maxX = -1, maxY = -1;
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
                if ((px[y * w + x] >>> 24) > 8)
                {
                    if (x < minX) minX = x;
                    if (x > maxX) maxX = x;
                    if (y < minY) minY = y;
                    if (y > maxY) maxY = y;
                }
        if (maxX < 0 || (minX == 0 && minY == 0 && maxX == w - 1 && maxY == h - 1)) return src;
        var crop = new CroppedBitmap(src, new Int32Rect(minX, minY, maxX - minX + 1, maxY - minY + 1));
        crop.Freeze();
        return crop;
    }

    static void DumpTree(SkinView view, Elements.SkinElement e, int depth, System.Text.StringBuilder sb)
    {
        var b = e == view ? new Rect(0, 0, e.Width, e.Height) : e.AbsoluteBounds;
        sb.Append(' ', depth * 2).Append($"<{e.Tag}{(e.Id != null ? " id=" + e.Id : "")}> abs=({b.X},{b.Y},{b.Width}x{b.Height}) rel=({e.Left},{e.Top}) z={e.ZIndex}");
        if (!e.Visible) sb.Append(" HIDDEN");
        foreach (var a in new[] { "backgroundimage", "image", "value", "horizontalalignment", "verticalalignment" })
            if (e.HasValue(a) && e.GetString(a).Length > 0) sb.Append($" {a}={e.GetString(a)}");
        sb.AppendLine();
        foreach (var c in e.Children) DumpTree(view, c, depth + 1, sb);
    }
}
