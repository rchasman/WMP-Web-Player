using System.IO;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using Windows.Media;
using Windows.Media.Control;

namespace WebSkinPlayer.Media;

public enum MediaStatus { None, Stopped, Paused, Playing, Changing }

/// <summary>Immutable view of what the selected web/desktop player is doing.</summary>
public sealed record MediaSnapshot
{
    public string Title { get; init; } = "";
    public string Artist { get; init; } = "";
    public string Album { get; init; } = "";
    public string AlbumArtist { get; init; } = "";
    public string SourceApp { get; init; } = "";
    public string SourceDisplayName { get; init; } = "";
    public MediaStatus Status { get; init; }
    public TimeSpan Duration { get; init; }
    public bool CanPlay { get; init; }
    public bool CanPause { get; init; }
    public bool CanStop { get; init; }
    public bool CanNext { get; init; }
    public bool CanPrevious { get; init; }
    public bool CanSeek { get; init; }
    public bool CanFastForward { get; init; }
    public bool CanRewind { get; init; }
    public bool CanShuffle { get; init; }
    public bool CanRepeat { get; init; }
    public bool Shuffle { get; init; }
    public bool Repeat { get; init; }
    public BitmapSource? Artwork { get; init; }
    public bool HasSession => SourceApp.Length > 0;

    public static readonly MediaSnapshot Empty = new();
}

public sealed record MediaSessionInfo(string AppId, string DisplayName, MediaStatus Status, string Title);

/// <summary>
/// Follows the Windows System Media Transport Controls sessions (Chrome, Edge, Spotify, ...)
/// and exposes the active one as a WMP-like transport. All events are raised on the UI dispatcher.
/// </summary>
public sealed class MediaBridge : IDisposable
{
    readonly Dispatcher _ui;
    GlobalSystemMediaTransportControlsSessionManager? _manager;
    GlobalSystemMediaTransportControlsSession? _session;
    GlobalSystemMediaTransportControlsSessionTimelineProperties? _timeline;
    DateTimeOffset _timelineFetched;
    int _refreshVersion;
    // After a seek, report the requested position until the source publishes a newer timeline.
    TimeSpan? _seekTarget;
    DateTimeOffset _seekAt;
    string _artworkKey = "";
    BitmapSource? _artwork;

    public MediaSnapshot Current { get; private set; } = MediaSnapshot.Empty;
    /// <summary>App id the user pinned; null means follow whatever is playing.</summary>
    public string? PreferredApp
    {
        get => _preferredApp;
        set { if (!string.Equals(value, _preferredApp, StringComparison.OrdinalIgnoreCase)) _preferredSeen = default; _preferredApp = value; }
    }
    string? _preferredApp;

    public event Action? Changed;
    public event Action? TrackChanged;
    public event Action? SessionsChanged;

    MediaBridge(Dispatcher ui) => _ui = ui;

    public static async Task<MediaBridge> CreateAsync(Dispatcher ui)
    {
        var bridge = new MediaBridge(ui);
        try
        {
            bridge._manager = await GlobalSystemMediaTransportControlsSessionManager.RequestAsync();
            bridge._manager.SessionsChanged += (_, _) => bridge.Post(() => { bridge.SelectSession(); bridge.SessionsChanged?.Invoke(); });
            bridge._manager.CurrentSessionChanged += (_, _) => bridge.Post(bridge.SelectSession);
            bridge.SelectSession();
        }
        catch (Exception ex)
        {
            Log.Write("SMTC unavailable: " + ex.Message);
        }
        return bridge;
    }

    void Post(Action a) => _ui.BeginInvoke(a);

    public IReadOnlyList<MediaSessionInfo> GetSessions()
    {
        var list = new List<MediaSessionInfo>();
        if (_manager == null) return list;
        foreach (var s in _manager.GetSessions())
        {
            string title = ReferenceEquals(s, _session) ? Current.Title : "";
            list.Add(new MediaSessionInfo(s.SourceAppUserModelId, FriendlyAppName(s.SourceAppUserModelId), Map(SafeStatus(s)), title));
        }
        return list;
    }

    /// <summary>Re-evaluate which session to follow (after <see cref="PreferredApp"/> changes).</summary>
    public void Reselect() => SelectSession();

    /// <summary>The app of the session followed (kept, as a session that has gone can't be asked).</summary>
    string? _sessionApp;
    /// <summary>When the pinned app last had a media session.</summary>
    DateTime _preferredSeen;
    DispatcherTimer? _preferredWait;
    static readonly TimeSpan PreferredGrace = TimeSpan.FromSeconds(15);

    void SelectSession()
    {
        if (_manager == null) return;
        var sessions = _manager.GetSessions();
        GlobalSystemMediaTransportControlsSession? pick = null;
        bool waiting = false;

        if (PreferredApp != null)
        {
            pick = sessions.FirstOrDefault(s => string.Equals(s.SourceAppUserModelId, PreferredApp, StringComparison.OrdinalIgnoreCase));
            // Following it until now counts as seeing it: this is the moment it went away.
            if (pick != null || string.Equals(_sessionApp, PreferredApp, StringComparison.OrdinalIgnoreCase)) _preferredSeen = DateTime.UtcNow;
            // The pinned app's session can vanish for a moment and come back (Chrome does at some SoundCloud song
            // changes): wait for it rather than jump to whatever else is playing meanwhile.
            if (pick == null && DateTime.UtcNow - _preferredSeen < PreferredGrace)
            {
                waiting = true;
                if (_preferredWait == null)
                {
                    _preferredWait = new DispatcherTimer(DispatcherPriority.Normal, _ui);
                    _preferredWait.Tick += (_, _) => { _preferredWait.Stop(); SelectSession(); };
                }
                _preferredWait.Stop();
                _preferredWait.Interval = PreferredGrace - (DateTime.UtcNow - _preferredSeen) + TimeSpan.FromMilliseconds(50);
                _preferredWait.Start();
            }
        }

        if (pick == null && !waiting)
        {
            // Stay on the current session while it is playing; otherwise jump to whatever is playing.
            bool currentPlaying = _session != null && sessions.Contains(_session) && SafeStatus(_session) == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing;
            if (currentPlaying) pick = _session;
            else
            {
                pick = sessions.FirstOrDefault(s => SafeStatus(s) == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing)
                    ?? (_session != null && sessions.Contains(_session) ? _session : null)
                    ?? SafeCurrent()
                    ?? sessions.FirstOrDefault();
            }
        }

        if (!ReferenceEquals(pick, _session))
        {
            if (_session != null)
            {
                _session.MediaPropertiesChanged -= OnMediaProps;
                _session.PlaybackInfoChanged -= OnPlayback;
                _session.TimelinePropertiesChanged -= OnTimeline;
            }
            _session = pick;
            _sessionApp = pick?.SourceAppUserModelId;
            _timeline = null;
            if (_session != null)
            {
                _session.MediaPropertiesChanged += OnMediaProps;
                _session.PlaybackInfoChanged += OnPlayback;
                _session.TimelinePropertiesChanged += OnTimeline;
            }
            Log.Write("Media session: " + (_sessionApp ?? "(none)") + (waiting ? $" (waiting for {PreferredApp} to come back)" : ""));
        }
        _ = RefreshAsync();
    }

    GlobalSystemMediaTransportControlsSession? SafeCurrent()
    {
        try { return _manager?.GetCurrentSession(); } catch { return null; }
    }

    static GlobalSystemMediaTransportControlsSessionPlaybackStatus SafeStatus(GlobalSystemMediaTransportControlsSession s)
    {
        try { return s.GetPlaybackInfo()?.PlaybackStatus ?? GlobalSystemMediaTransportControlsSessionPlaybackStatus.Closed; }
        catch { return GlobalSystemMediaTransportControlsSessionPlaybackStatus.Closed; }
    }

    void OnMediaProps(GlobalSystemMediaTransportControlsSession s, MediaPropertiesChangedEventArgs e) => Post(() => _ = RefreshAsync());
    void OnTimeline(GlobalSystemMediaTransportControlsSession s, TimelinePropertiesChangedEventArgs e) => Post(() => _ = RefreshAsync());
    void OnPlayback(GlobalSystemMediaTransportControlsSession s, PlaybackInfoChangedEventArgs e) => Post(() =>
    {
        // A different app may have started playing; re-evaluate which session we follow.
        if (PreferredApp == null && SafeStatus(s) != GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing)
            SelectSession();
        else
            _ = RefreshAsync();
    });

    async Task RefreshAsync()
    {
        int version = ++_refreshVersion;
        var s = _session;
        if (s == null)
        {
            Publish(MediaSnapshot.Empty);
            return;
        }

        GlobalSystemMediaTransportControlsSessionMediaProperties? props = null;
        try { props = await s.TryGetMediaPropertiesAsync(); } catch (Exception ex) { Log.Write("Media props failed: " + ex.Message); }
        if (version != _refreshVersion) return;

        GlobalSystemMediaTransportControlsSessionPlaybackInfo? info = null;
        try { info = s.GetPlaybackInfo(); } catch { }
        try { _timeline = s.GetTimelineProperties(); _timelineFetched = DateTimeOffset.Now; } catch { _timeline = null; }
        if (_seekTarget != null && _timeline != null && _timeline.LastUpdatedTime > _seekAt) _seekTarget = null;

        var controls = info?.Controls;
        var duration = _timeline != null ? _timeline.EndTime - _timeline.StartTime : TimeSpan.Zero;
        if (duration < TimeSpan.Zero) duration = TimeSpan.Zero;

        string key = $"{s.SourceAppUserModelId}|{props?.Title}|{props?.Artist}|{props?.AlbumTitle}";
        if (key != _artworkKey)
        {
            _artworkKey = key;
            _artwork = null;
            if (props?.Thumbnail != null)
            {
                var art = await LoadArtworkAsync(props.Thumbnail);
                if (version != _refreshVersion && key != _artworkKey) return;
                _artwork = art;
            }
        }

        var snap = new MediaSnapshot
        {
            Title = props?.Title ?? "",
            Artist = props?.Artist ?? "",
            Album = props?.AlbumTitle ?? "",
            AlbumArtist = props?.AlbumArtist ?? "",
            SourceApp = s.SourceAppUserModelId ?? "",
            SourceDisplayName = FriendlyAppName(s.SourceAppUserModelId ?? ""),
            Status = Map(info?.PlaybackStatus ?? GlobalSystemMediaTransportControlsSessionPlaybackStatus.Closed),
            Duration = duration,
            CanPlay = controls?.IsPlayEnabled ?? false,
            CanPause = controls?.IsPauseEnabled ?? false,
            CanStop = (controls?.IsStopEnabled ?? false) || (controls?.IsPauseEnabled ?? false),
            CanNext = controls?.IsNextEnabled ?? false,
            CanPrevious = controls?.IsPreviousEnabled ?? false,
            CanSeek = (controls?.IsPlaybackPositionEnabled ?? false) && duration > TimeSpan.Zero,
            CanFastForward = (controls?.IsFastForwardEnabled ?? false) || ((controls?.IsPlaybackPositionEnabled ?? false) && duration > TimeSpan.Zero),
            CanRewind = (controls?.IsRewindEnabled ?? false) || ((controls?.IsPlaybackPositionEnabled ?? false) && duration > TimeSpan.Zero),
            CanShuffle = controls?.IsShuffleEnabled ?? false,
            CanRepeat = controls?.IsRepeatEnabled ?? false,
            Shuffle = info?.IsShuffleActive ?? false,
            Repeat = info?.AutoRepeatMode is MediaPlaybackAutoRepeatMode.List or MediaPlaybackAutoRepeatMode.Track,
            Artwork = _artwork,
        };
        Publish(snap);
    }

    void Publish(MediaSnapshot snap)
    {
        if (_simulated == null) Show(snap);
    }

    Func<TimeSpan>? _simulated;

    /// <summary>
    /// Test hook: shows <paramref name="snap"/> at <paramref name="position"/> instead of the system's media
    /// sessions, so a test run can play a song on a machine with no sound device.
    /// </summary>
    public void Simulate(MediaSnapshot snap, Func<TimeSpan> position)
    {
        _simulated = position;
        Show(snap);
    }

    void Show(MediaSnapshot snap)
    {
        var old = Current;
        Current = snap;
        bool track = old.Title != snap.Title || old.Artist != snap.Artist || old.SourceApp != snap.SourceApp || !ReferenceEquals(old.Artwork, snap.Artwork);
        if (track) TrackChanged?.Invoke();
        Changed?.Invoke();
    }

    static async Task<BitmapSource?> LoadArtworkAsync(Windows.Storage.Streams.IRandomAccessStreamReference reference)
    {
        try
        {
            using var stream = await reference.OpenReadAsync();
            using var net = stream.AsStreamForRead();
            var ms = new MemoryStream();
            await net.CopyToAsync(ms);
            ms.Position = 0;
            var bmp = new BitmapImage();
            bmp.BeginInit();
            bmp.CacheOption = BitmapCacheOption.OnLoad;
            bmp.StreamSource = ms;
            bmp.EndInit();
            bmp.Freeze();
            return bmp;
        }
        catch (Exception ex)
        {
            Log.Write("Artwork load failed: " + ex.Message);
            return null;
        }
    }

    /// <summary>Current position, extrapolated between the sparse updates browsers send.</summary>
    public TimeSpan Position
    {
        get
        {
            if (_simulated is { } simulated) return simulated();
            var t = _timeline;
            if (t == null) return TimeSpan.Zero;
            if (_seekTarget is { } target)
            {
                var since = DateTimeOffset.Now - _seekAt;
                if (since < TimeSpan.FromSeconds(4))
                    return Current.Status == MediaStatus.Playing ? target + since : target;
                _seekTarget = null;
            }
            var pos = t.Position - t.StartTime;
            if (Current.Status == MediaStatus.Playing)
            {
                var since = t.LastUpdatedTime.Year > 2000 ? DateTimeOffset.Now - t.LastUpdatedTime : DateTimeOffset.Now - _timelineFetched;
                if (since > TimeSpan.Zero) pos += since;
            }
            var dur = t.EndTime - t.StartTime;
            if (dur > TimeSpan.Zero && pos > dur) pos = dur;
            return pos < TimeSpan.Zero ? TimeSpan.Zero : pos;
        }
    }

    static MediaStatus Map(GlobalSystemMediaTransportControlsSessionPlaybackStatus s) => s switch
    {
        GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing => MediaStatus.Playing,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus.Paused => MediaStatus.Paused,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus.Stopped => MediaStatus.Stopped,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus.Changing => MediaStatus.Changing,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus.Opened => MediaStatus.Stopped,
        _ => MediaStatus.None,
    };

    public static string FriendlyAppName(string appId)
    {
        string id = appId.ToLowerInvariant();
        if (id.Contains("chrome")) return "Google Chrome";
        if (id.Contains("msedge")) return "Microsoft Edge";
        if (id.Contains("spotify")) return "Spotify";
        if (id.Contains("firefox") || id == "308046b0af4a39cb") return "Firefox";
        if (id.Contains("brave")) return "Brave";
        if (id.Contains("opera")) return "Opera";
        if (id.Contains("zunemusic")) return "Media Player";
        int bang = appId.IndexOf('!');
        if (bang >= 0) return appId[(bang + 1)..];
        return Path.GetFileNameWithoutExtension(appId);
    }

    /// <summary>Best-guess process name owning the session, used for per-app volume.</summary>
    public static string ProcessNameForApp(string appId)
    {
        string id = appId.ToLowerInvariant();
        if (id.Contains("chrome")) return "chrome";
        if (id.Contains("msedge")) return "msedge";
        if (id.Contains("spotify")) return "spotify";
        if (id.Contains("firefox") || id == "308046b0af4a39cb") return "firefox";
        if (id.Contains("brave")) return "brave";
        if (id.Contains("opera")) return "opera";
        int bang = id.IndexOf('!');
        if (bang >= 0) id = id[(bang + 1)..];
        return Path.GetFileNameWithoutExtension(id);
    }

    // ---- Commands -------------------------------------------------------------------------

    public void Play() => Run(s => s.TryPlayAsync().AsTask());
    public void Pause() => Run(s => s.TryPauseAsync().AsTask());
    public void TogglePlayPause() => Run(s => s.TryTogglePlayPauseAsync().AsTask());
    public void Next() => Run(s => s.TrySkipNextAsync().AsTask());
    public void Previous() => Run(s => s.TrySkipPreviousAsync().AsTask());

    public void Stop() => Run(async s =>
    {
        // Web players rarely implement "stop"; emulate WMP by pausing and rewinding.
        if (await s.TryStopAsync()) return true;
        bool ok = await s.TryPauseAsync();
        if (Current.CanSeek) await s.TryChangePlaybackPositionAsync(0);
        return ok;
    });

    public void FastForward() => Run(async s =>
    {
        if (s.GetPlaybackInfo()?.Controls?.IsFastForwardEnabled == true && await s.TryFastForwardAsync()) return true;
        return await SeekByAsync(s, TimeSpan.FromSeconds(10));
    });

    public void Rewind() => Run(async s =>
    {
        if (s.GetPlaybackInfo()?.Controls?.IsRewindEnabled == true && await s.TryRewindAsync()) return true;
        return await SeekByAsync(s, TimeSpan.FromSeconds(-10));
    });

    public void Seek(TimeSpan position)
    {
        if (position < TimeSpan.Zero) position = TimeSpan.Zero;
        _seekTarget = position;
        _seekAt = DateTimeOffset.Now;
        Run(s =>
        {
            var start = _timeline?.StartTime ?? TimeSpan.Zero;
            return s.TryChangePlaybackPositionAsync((start + position).Ticks).AsTask();
        });
    }

    public void SetShuffle(bool on) => Run(s => s.TryChangeShuffleActiveAsync(on).AsTask());
    public void SetRepeat(bool on) => Run(s => s.TryChangeAutoRepeatModeAsync(on ? MediaPlaybackAutoRepeatMode.List : MediaPlaybackAutoRepeatMode.None).AsTask());

    Task<bool> SeekByAsync(GlobalSystemMediaTransportControlsSession s, TimeSpan delta)
    {
        var start = _timeline?.StartTime ?? TimeSpan.Zero;
        var target = Position + delta;
        if (target < TimeSpan.Zero) target = TimeSpan.Zero;
        if (Current.Duration > TimeSpan.Zero && target > Current.Duration) target = Current.Duration;
        return s.TryChangePlaybackPositionAsync((start + target).Ticks).AsTask();
    }

    void Run(Func<GlobalSystemMediaTransportControlsSession, Task<bool>> action)
    {
        var s = _session;
        if (s == null || Sandbox.Enabled) return;
        _ = Task.Run(async () =>
        {
            try
            {
                bool ok = await action(s);
                if (!ok) Log.Write("Media command was not accepted by " + s.SourceAppUserModelId);
            }
            catch (Exception ex) { Log.Write("Media command failed: " + ex.Message); }
            Post(() => _ = RefreshAsync());
        });
    }

    public void Dispose()
    {
        if (_session != null)
        {
            _session.MediaPropertiesChanged -= OnMediaProps;
            _session.PlaybackInfoChanged -= OnPlayback;
            _session.TimelinePropertiesChanged -= OnTimeline;
        }
        _session = null;
    }
}
