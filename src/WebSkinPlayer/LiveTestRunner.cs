using System.IO;
using System.Text;
using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using WebSkinPlayer.Media;
using WebSkinPlayer.Skins;
using WebSkinPlayer.Skins.Elements;

namespace WebSkinPlayer;

/// <summary>
/// "--test-live &lt;folder|file&gt; &lt;outDir&gt; [seconds]": opens each skin for real but hidden and
/// sandboxed (scripts, timers, GIF animations, player events all run; nothing reaches the real
/// player) with a song playing, saves every window it opens, measures how much of every visualizer
/// area is actually visible, and checks that every control it shows can be clicked. Then the song
/// pauses, resumes, changes, jumps and stops, and the windows are saved and checked again after the
/// pause, the change and the stop, with the script errors each event raised. Returns the number of
/// controls out of reach.
/// </summary>
public static class LiveTestRunner
{
    /// <summary>The song the followed app plays, as skins are used and as their authors took their preview screenshots.</summary>
    static readonly MediaSnapshot Glue = new()
    {
        Title = "Glue", Artist = "Bicep", Album = "Bicep", SourceApp = "Spotify", SourceDisplayName = "Spotify",
        Status = MediaStatus.Playing, Duration = TimeSpan.FromSeconds(269),
        CanPlay = true, CanPause = true, CanStop = true, CanNext = true, CanPrevious = true, CanSeek = true,
        CanFastForward = true, CanRewind = true, CanShuffle = true, CanRepeat = true,
    };

    static MediaSnapshot _song = Glue;
    /// <summary>Where the song is, in seconds.</summary>
    static double _position;

    /// <summary>What the followed app does after the first pass, in this order.</summary>
    enum SongEvent { Paused, Resumed, Next, Seeked, Stopped }

    /// <summary>
    /// Tenths of a second to run after an event, long enough for the player's holds (1.5 s for a pause,
    /// 15 s for a stop it did not ask for) to let the skin see it, and whether the windows are saved and
    /// checked after it.
    /// </summary>
    static (int Steps, bool Checked) Plan(SongEvent e) => e switch
    {
        SongEvent.Paused => (25, true),
        SongEvent.Resumed or SongEvent.Seeked => (10, false),
        SongEvent.Next => (20, true),
        _ => (160, true),
    };

    static void Apply(SongEvent e)
    {
        switch (e)
        {
            case SongEvent.Paused: _song = _song with { Status = MediaStatus.Paused }; break;
            case SongEvent.Resumed: _song = _song with { Status = MediaStatus.Playing }; break;
            case SongEvent.Next:
                _song = _song with { Title = "Opal", Artist = "Four Tet", Album = "New Energy", Duration = TimeSpan.FromSeconds(180) };
                _position = 0;
                break;
            case SongEvent.Seeked: _position = 150; break;
            // WMP's stop: the followed app stops and the player shows Stopped.
            default: _song = _song with { Status = MediaStatus.Stopped }; _position = 0; break;
        }
        App.Services.Media.Simulate(_song, () => TimeSpan.FromSeconds(_position));
    }

    public static async Task<int> RunAsync(string source, string outDir, double seconds)
    {
        Sandbox.Enabled = true;
        Directory.CreateDirectory(outDir);
        var files = Directory.Exists(source)
            ? Directory.EnumerateFiles(source, "*.wmz").OrderBy(f => f).ToList()
            : new List<string> { source };
        var report = new StringBuilder();
        var counts = new Dictionary<string, int>();
        int faults = 0, withEventErrors = 0;
        string? phase = null;
        var eventErrors = new List<string>();
        void OnLog(string line)
        {
            if (phase != null && line.Contains("] script error in ", StringComparison.Ordinal)) eventErrors.Add($"after {phase}: {line[(line.IndexOf(' ') + 1)..]}");
        }
        Log.Written += OnLog;
        foreach (var file in files)
        {
            string name = Path.GetFileNameWithoutExtension(file);
            SkinTheme? theme = null;
            var lines = new List<string>();
            string status = "NO-VIS";
            phase = null;
            eventErrors.Clear();
            try
            {
                (_song, _position) = (Glue, 40);
                App.Services.Media.Simulate(_song, () => TimeSpan.FromSeconds(_position));
                var applied = theme = SkinTheme.Load(file);
                applied.OpenMainView();
                // Hidden windows never paint, so draw every 100 ms like the real window would (this is what
                // starts and advances GIF animations), with the song moving on while it plays.
                async Task Run(int steps)
                {
                    for (int i = 0; i < steps; i++)
                    {
                        if (_song.Status == MediaStatus.Playing) _position += 0.1;
                        await Task.Delay(100);
                        foreach (var v in applied.OpenViews.ToList()) Render(v);
                    }
                }
                void SaveAndCheck(string? after)
                {
                    foreach (var (view, i) in applied.OpenViews.ToList().Select((v, i) => (v, i)))
                    {
                        string id = System.Text.RegularExpressions.Regex.Replace(view.Id ?? $"view{i}", "[^A-Za-z0-9_-]", "_");
                        string label = after == null ? id : $"{id}.{after}";
                        Save(Render(view), Path.Combine(outDir, $"{name}.{label}.png"));
                        view.HeldAt = DateTime.UtcNow;
                        try
                        {
                            foreach (var fault in ControlsOutOfReach(view))
                            {
                                faults++;
                                lines.Add($"    FAULT view {label}: {fault}");
                            }
                        }
                        finally { view.HeldAt = null; }
                    }
                }
                await Run((int)Math.Round(seconds * 10));
                // Past the mask rebuild limit, so the windows are cut as they are now.
                await Task.Delay(150);
                foreach (var view in applied.OpenViews.ToList())
                {
                    var result = Analyze(view, lines);
                    if (Rank(result) > Rank(status)) status = result;
                }
                SaveAndCheck(null);
                foreach (var e in Enum.GetValues<SongEvent>())
                {
                    phase = e.ToString().ToLowerInvariant();
                    Apply(e);
                    var (steps, check) = Plan(e);
                    await Run(steps);
                    if (!check) continue;
                    await Task.Delay(150);
                    SaveAndCheck(phase);
                }
            }
            catch (Exception ex)
            {
                status = "FAILED";
                lines.Add($"    {ex.GetType().Name}: {ex.Message}");
            }
            finally
            {
                phase = null;
                try { theme?.Close(); } catch { }
            }
            if (eventErrors.Count > 0)
            {
                withEventErrors++;
                lines.AddRange(eventErrors.Take(8).Select(e => $"    EVENT {e}"));
            }
            counts[status] = counts.GetValueOrDefault(status) + 1;
            report.AppendLine($"{status,-8} {name}");
            foreach (var l in lines) report.AppendLine(l);
            await Task.Delay(50);
        }
        Log.Written -= OnLog;
        report.Insert(0, $"{files.Count} skins after {seconds}s live: " + string.Join(", ", counts.OrderBy(k => k.Key).Select(k => $"{k.Value} {k.Key}")) +
            $"; {faults} control(s) out of reach; {withEventErrors} with script errors after song events" +
            Environment.NewLine + "OK = visualizer visible; PARTLY/COVERED = drawn under other elements; HIDDEN/FADED = hidden or faded out by the skin until a button or mode shows it" +
            Environment.NewLine + Environment.NewLine);
        File.WriteAllText(Path.Combine(outDir, "live-report.txt"), report.ToString());
        return faults;
    }

    /// <summary>
    /// Controls that show in the window but take no click at any pixel they show: something transparent to
    /// the eye lies over them, or the window is see-through there. A control another part covers (a closed
    /// drawer, an intro screen) shows nowhere and is left alone, and so is what shows through a slider: a
    /// slider takes clicks across its whole rectangle, like a Windows trackbar. Both pictures are the window
    /// as cut, so a pixel the window cuts either way is not one the control shows.
    /// </summary>
    static List<string> ControlsOutOfReach(SkinView view)
    {
        var faults = new List<string>();
        int w = Math.Min(4000, view.Width), h = Math.Min(4000, view.Height);
        if (w <= 0 || h <= 0) return faults;
        var with = view.RenderWindow();
        foreach (var e in view.AllElements.Where(e => TakesClicks(e) && e.IsEffectivelyVisible).ToList())
        {
            var group = e is GroupButtonElement && e.Parent is ButtonGroupElement g ? g : null;
            SkinElement owner = group ?? e;
            var b = owner.AbsoluteBounds;
            var area = b;
            for (var p = owner.Parent; p != null; p = p.Parent) if (p.ClipsChildren) area.Intersect(p.AbsoluteBounds);
            area.Intersect(new Rect(0, 0, w, h));
            if (area.IsEmpty) continue;
            int x0 = (int)Math.Ceiling(area.Left), y0 = (int)Math.Ceiling(area.Top), x1 = (int)Math.Floor(area.Right), y1 = (int)Math.Floor(area.Bottom);
            if (x1 <= x0 || y1 <= y0) continue;
            var without = view.RenderWindow(e);
            var takers = new Dictionary<string, int>();
            bool reached = false;
            for (int y = y0; y < y1 && !reached; y++)
                for (int x = x0; x < x1 && !reached; x++)
                {
                    var local = new Point(x + 0.5 - b.X, y + 0.5 - b.Y);
                    bool own = group != null ? group.ElementAt(local) == e : e.HitTest(local);
                    int j = y * w + x;
                    if (!own || with[j] == without[j]) continue;
                    var taker = view.HitTestPoint(new Point(x + 0.5, y + 0.5));
                    if (taker != null && taker != e && taker is SliderElement) continue;
                    bool opaque = (with[j] >>> 24) > 0;
                    if (taker == e && opaque) { reached = true; continue; }
                    string by = opaque && taker != null ? Label(taker) : "see-through window";
                    takers[by] = takers.GetValueOrDefault(by) + 1;
                }
            if (reached || takers.Count == 0) continue;
            var most = takers.MaxBy(t => t.Value);
            faults.Add($"{Label(e)} shows in {takers.Values.Sum()} pixels but can't be clicked: {most.Key} takes them");
        }
        return faults;
    }

    static readonly string[] ClickHandlers = { "onclick", "onmousedown", "onmouseup", "ondblclick" };

    /// <summary>
    /// A control meant to be clicked now: enabled, not passing clicks through (Scooby-Doo's second seek bar
    /// mirrors the first), nor in a button group that does, and a button only when clicking it does something.
    /// </summary>
    static bool TakesClicks(SkinElement e) =>
        e is ButtonElement or GroupButtonElement or SliderElement or CustomSliderElement
        && !e.PassesClicks && e.Parent is not ButtonGroupElement { PassesClicks: true } && e.Enabled
        && (e is SliderElement or CustomSliderElement || ButtonPresets.For(e.Tag) != null || ClickHandlers.Any(e.HasHandler));

    static string Label(SkinElement e) => e.Id != null ? $"<{e.Tag} id={e.Id}>" : $"<{e.Tag}> at {e.AbsoluteBounds.X:0},{e.AbsoluteBounds.Y:0}";

    static int Rank(string s) => s switch { "OK" => 5, "PARTLY" => 4, "COVERED" => 3, "HIDDEN" or "FADED" => 2, "FAILED" => 1, _ => 0 };

    static string Analyze(SkinView view, List<string> lines)
    {
        var fx = view.AllElements.OfType<EffectsElement>().Where(e => e is not VideoElement v || v.ShowsVisualization).ToList();
        if (fx.Count == 0) return "NO-VIS";
        EffectsElement.MarkerMode = true;
        int[] px;
        int w = Math.Max(1, view.Width), h = Math.Max(1, view.Height);
        try { px = Pixels(Render(view), out w, out h); }
        finally { EffectsElement.MarkerMode = false; }
        int marker = (EffectsElement.MarkerColor.R << 16) | (EffectsElement.MarkerColor.G << 8) | EffectsElement.MarkerColor.B;
        string best = "NO-VIS";
        foreach (var e in fx)
        {
            var b = e.AbsoluteBounds;
            b.Intersect(new Rect(0, 0, w, h));
            string state;
            double share = 0;
            if (!e.IsEffectivelyVisible) state = "HIDDEN";
            else if (b.IsEmpty || b.Width < 1 || b.Height < 1) state = "HIDDEN";
            else
            {
                int hit = 0, total = 0;
                for (int y = (int)b.Top; y < (int)b.Bottom; y++)
                    for (int x = (int)b.Left; x < (int)b.Right; x++)
                    {
                        total++;
                        int c = px[y * w + x];
                        if ((c >>> 24) == 255 && (c & 0xFFFFFF) == marker) hit++;
                    }
                share = total > 0 ? hit / (double)total : 0;
                double alpha = 1;
                for (SkinElement? p = e; p != null; p = p.Parent) alpha *= p.Opacity;
                // A visualizer the skin's own script faded out is shown on demand, like a hidden one.
                state = share >= 0.6 ? "OK" : alpha < 0.1 ? "FADED" : share >= 0.1 ? "PARTLY" : "COVERED";
            }
            lines.Add($"    view {view.Id ?? "(main)"}: {e.Tag}{(e.Id != null ? "#" + e.Id : "")} at ({b.X:0},{b.Y:0} {b.Width:0}x{b.Height:0}) {state}{(state is "OK" or "PARTLY" or "COVERED" or "FADED" ? $" {share:P0} visible" : "")}");
            if (state is "PARTLY" or "COVERED" or "FADED") lines.Add("      " + Diagnose(view, e, b));
            if (state == "HIDDEN") lines.Add("      " + WhyHidden(view, e));
            if (Rank(state) > Rank(best)) best = state;
        }
        return best;
    }

    /// <summary>Which element hides the visualizer, and whether the skin's own code ever shows it again.</summary>
    static string WhyHidden(SkinView view, SkinElement fx)
    {
        SkinElement? gate = null;
        for (SkinElement? p = fx; p != null && p is not SkinView; p = p.Parent) if (!p.Visible) gate = p;
        if (gate == null) return "zero size";
        string name = gate.Id ?? "";
        if (name.Length == 0) return $"hidden by an unnamed {gate.Tag} (nothing can show it by name)";
        var pkg = view.Theme.Package;
        var code = string.Join("\n", pkg.Files.Where(f => f.EndsWith(".js", StringComparison.OrdinalIgnoreCase) || f.EndsWith(".wms", StringComparison.OrdinalIgnoreCase))
            .Select(f => { try { return pkg.ReadText(f); } catch { return ""; } }));
        // e.g. visMask.visible = true, visMask.visible=!visMask.visible, visible="wmpprop:...", visMask.show()
        bool shown = System.Text.RegularExpressions.Regex.IsMatch(code,
            $@"\b{System.Text.RegularExpressions.Regex.Escape(name)}\s*\.\s*(visible\s*=(?!=)|show\s*\()", System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        bool bound = (gate.Node.Attr("visible") ?? "").Contains(':');
        return $"hidden by {gate.Tag}#{name}; " + (shown ? "the skin's script shows it on demand" : bound ? "its visibility is bound to player state" : "NOTHING in the skin shows it again");
    }

    /// <summary>Why a visualizer isn't showing: its own fade, and what is drawn over its centre.</summary>
    static string Diagnose(SkinView view, SkinElement fx, Rect b)
    {
        double alpha = 1;
        var faded = new List<string>();
        for (SkinElement? p = fx; p != null; p = p.Parent)
        {
            alpha *= p.Opacity;
            if (p.Opacity < 0.99) faded.Add($"{p.Tag}{(p.Id != null ? "#" + p.Id : "")}={p.Opacity * 255:0}");
        }
        var order = new List<SkinElement>();
        void Walk(SkinElement e)
        {
            var (behind, above) = e.ChildrenBehindAndAbove;
            foreach (var c in behind) Walk(c);
            order.Add(e);
            foreach (var c in above) Walk(c);
        }
        Walk(view);
        int at = order.IndexOf(fx);
        var centre = new Point(b.X + b.Width / 2, b.Y + b.Height / 2);
        var over = order.Skip(at + 1)
            .Where(e => e.IsEffectivelyVisible && e.AbsoluteBounds.Contains(centre) && !IsAncestor(fx, e) && !IsAncestor(e, fx))
            .Reverse().Take(3)
            .Select(e => $"{e.Tag}{(e.Id != null ? "#" + e.Id : "")} z={e.ZIndex} {Describe(e)}");
        return $"opacity {alpha:0.##}{(faded.Count > 0 ? " (" + string.Join(", ", faded) + ")" : "")}, player {App.Services.Player.Status}; drawn over it: " + (string.Join(" | ", over) is { Length: > 0 } s ? s : "nothing");
    }

    static bool IsAncestor(SkinElement a, SkinElement of) { for (var p = of.Parent; p != null; p = p.Parent) if (p == a) return true; return false; }

    static string Describe(SkinElement e)
    {
        var parts = new List<string>();
        foreach (var key in new[] { "backgroundimage", "image", "backgroundcolor", "transparencycolor", "clippingcolor" })
            if (e.GetString(key) is { Length: > 0 } v && v != "none") parts.Add($"{key}={v}");
        return string.Join(" ", parts);
    }

    static BitmapSource Render(SkinView view)
    {
        int w = Math.Clamp(view.Width, 1, 4000), h = Math.Clamp(view.Height, 1, 4000);
        var dv = new DrawingVisual();
        using (var dc = dv.RenderOpen()) view.RenderTree(dc);
        var rtb = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        rtb.Render(dv);
        rtb.Freeze();
        return rtb;
    }

    static int[] Pixels(BitmapSource b, out int w, out int h)
    {
        w = b.PixelWidth; h = b.PixelHeight;
        var px = new int[w * h];
        b.CopyPixels(px, w * 4, 0);
        return px;
    }

    static void Save(BitmapSource b, string path)
    {
        var enc = new PngBitmapEncoder();
        enc.Frames.Add(BitmapFrame.Create(b));
        using var fs = File.Create(path);
        enc.Save(fs);
    }
}
