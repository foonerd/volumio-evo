(function () {
  var STORAGE_KEY = 'evoVideoOverlaySize';

  function destroyHls(wrap) {
    if (wrap && wrap._hls) {
      try {
        wrap._hls.destroy();
      } catch (_e) {}
      wrap._hls = null;
    }
  }

  /** ffmpeg may start slightly after pushState; GET /hls/.../index.m3u8 can 404 until the playlist exists. */
  function waitForManifest(url, onReady, attempt) {
    attempt = attempt || 0;
    var maxAttempts = 100;
    var delayMs = 250;
    fetch(url, { method: 'GET', cache: 'no-store', credentials: 'same-origin' })
      .then(function (res) {
        if (res.ok) {
          onReady();
          return;
        }
        if (attempt < maxAttempts) {
          setTimeout(function () {
            waitForManifest(url, onReady, attempt + 1);
          }, delayMs);
        } else {
          onReady();
        }
      })
      .catch(function () {
        if (attempt < maxAttempts) {
          setTimeout(function () {
            waitForManifest(url, onReady, attempt + 1);
          }, delayMs);
        } else {
          onReady();
        }
      });
  }

  function overlayBaseCss() {
    return (
      'position:fixed;bottom:16px;right:16px;z-index:99998;background:#000;border-radius:10px;' +
      'box-shadow:0 8px 32px rgba(0,0,0,.55);overflow:hidden;display:none;flex-direction:column;'
    );
  }

  var SIZE_CSS = {
    sm: 'width:min(280px,38vw);max-height:28vh;',
    md: 'width:min(420px,46vw);max-height:38vh;',
    lg: 'width:min(560px,56vw);max-height:48vh;',
    max: 'width:min(92vw,1200px);max-height:85vh;',
  };

  function applyOverlaySize(wrap, preset) {
    var sz = SIZE_CSS[preset] || SIZE_CSS.md;
    wrap.style.cssText = overlayBaseCss() + sz + 'display:flex;';
    try {
      sessionStorage.setItem(STORAGE_KEY, preset);
    } catch (_e) {}
  }

  function readStoredPreset() {
    try {
      return sessionStorage.getItem(STORAGE_KEY) || 'md';
    } catch (_e) {
      return 'md';
    }
  }

  function ensureToolbar(wrap) {
    if (wrap.querySelector('.evo-vid-toolbar')) return;
    var tb = document.createElement('div');
    tb.className = 'evo-vid-toolbar';
    tb.style.cssText =
      'flex-shrink:0;display:flex;gap:6px;align-items:center;justify-content:flex-end;' +
      'flex-wrap:wrap;padding:6px 8px;background:linear-gradient(180deg,#2d2d2d,#141414);' +
      'border-bottom:1px solid #333;';

    function mk(label, preset) {
      var b = document.createElement('button');
      b.type = 'button';
      b.setAttribute('aria-label', label);
      b.textContent = label;
      b.style.cssText =
        'font:12px system-ui,-apple-system,sans-serif;padding:4px 10px;border-radius:6px;' +
        'border:1px solid #555;background:#383838;color:#eee;cursor:pointer;';
      b.onclick = function () {
        applyOverlaySize(wrap, preset);
      };
      tb.appendChild(b);
    }

    mk('S', 'sm');
    mk('M', 'md');
    mk('L', 'lg');
    mk('XL', 'max');

    var fs = document.createElement('button');
    fs.type = 'button';
    fs.setAttribute('aria-label', 'Fullscreen');
    fs.textContent = 'Full';
    fs.style.cssText =
      'font:14px system-ui,sans-serif;padding:4px 10px;border-radius:6px;border:1px solid #555;' +
      'background:#383838;color:#eee;cursor:pointer;margin-left:4px;';
    fs.onclick = function () {
      if (!document.fullscreenElement) {
        wrap.requestFullscreen &&
          wrap.requestFullscreen().catch(function () {});
      } else {
        document.exitFullscreen && document.exitFullscreen();
      }
    };
    tb.appendChild(fs);

    wrap.insertBefore(tb, wrap.firstChild);
  }

  /** Scenario 2 (`pushState.videoBrowserMuxed`): HLS carries **AAC** — unmute so the built-in sync is audible. */
  function applyBrowserMuxMute(v, browserMuxed) {
    if (!v) return;
    if (browserMuxed) {
      v.muted = false;
      v.removeAttribute('muted');
    } else {
      v.muted = true;
      v.setAttribute('muted', '');
    }
  }

  function buildHlsInstance() {
    return new window.Hls({
      enableWorker: true,
      lowLatencyMode: false,
      // Shorter segments + LAN: stay closer to the live edge without huge buffers (reduces drift vs pushState seek).
      liveSyncDurationCount: 2,
      liveMaxLatencyDurationCount: 10,
      maxBufferLength: 42,
      maxMaxBufferLength: 90,
      maxBufferHole: 0.35,
      maxFragLookUpTolerance: 0.25,
      nudgeOffset: 0.05,
      initialLiveManifestSize: 2,
      manifestLoadingMaxRetry: 12,
      levelLoadingMaxRetry: 12,
      fragLoadingMaxRetry: 16,
    });
  }

  function ensureVideo(url, browserMuxed) {
    var id = 'evo-video-companion-overlay';
    var wrap = document.getElementById(id);
    if (!wrap) {
      wrap = document.createElement('div');
      wrap.id = id;
      document.body.appendChild(wrap);
      applyOverlaySize(wrap, readStoredPreset());
      ensureToolbar(wrap);
    } else {
      ensureToolbar(wrap);
      if (wrap.style.display === 'none') {
        applyOverlaySize(wrap, readStoredPreset());
      }
    }

    var v = wrap.querySelector('video');
    if (!v) {
      v = document.createElement('video');
      v.setAttribute('playsinline', '');
      v.setAttribute('controls', '');
      v.style.cssText = 'width:100%;height:auto;display:block;background:#000;flex:1;min-height:0;';
      wrap.appendChild(v);
    }
    applyBrowserMuxMute(v, !!browserMuxed);

    if (wrap._evoUrl !== url) {
      destroyHls(wrap);
      wrap._evoUrl = url;
      waitForManifest(url, function attach() {
        if (wrap._evoUrl !== url) return;
        if (typeof window.Hls !== 'undefined' && window.Hls.isSupported()) {
          var hls = buildHlsInstance();
          wrap._hls = hls;
          hls.loadSource(url);
          hls.attachMedia(v);
        } else if (v.canPlayType('application/vnd.apple.mpegurl')) {
          v.src = url;
        } else {
          v.src = url;
        }
        try {
          v.play().catch(function () {});
        } catch (_e) {}
      });
    }
    wrap.style.display = 'flex';
    try {
      v.play().catch(function () {});
    } catch (_e) {}
  }

  function hideVideo() {
    var wrap = document.getElementById('evo-video-companion-overlay');
    if (!wrap) return;
    destroyHls(wrap);
    wrap._evoUrl = null;
    wrap.style.display = 'none';
    var v = wrap.querySelector('video');
    if (v) {
      try {
        v.pause();
        v.removeAttribute('src');
        v.load();
      } catch (_e) {}
    }
    if (document.fullscreenElement === wrap) {
      try {
        document.exitFullscreen();
      } catch (_e) {}
    }
  }

  function onState(state) {
    if (!state) return;
    var u = state.videoStreamUrl;
    if (u && state.status && state.status !== 'stop') {
      ensureVideo(u, !!state.videoBrowserMuxed);
    } else {
      hideVideo();
    }
  }

  function start() {
    var el = document.body;
    if (!el) return false;
    var inj = angular.element(el).injector();
    if (!inj) return false;
    var $rootScope = inj.get('$rootScope');
    $rootScope.$on('socket:pushState', function (_e, state) {
      onState(state);
    });
    try {
      var ps = inj.get('playerService');
      if (ps && ps.state) onState(ps.state);
    } catch (_e) {}
    return true;
  }

  function tryStart() {
    if (start()) return;
    var tries = 0;
    var id = setInterval(function () {
      tries += 1;
      if (start() || tries > 120) clearInterval(id);
    }, 50);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', tryStart);
  } else {
    tryStart();
  }
})();
