(function () {
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

  function ensureVideo(url) {
    var id = 'evo-video-companion-overlay';
    var wrap = document.getElementById(id);
    if (!wrap) {
      wrap = document.createElement('div');
      wrap.id = id;
      wrap.style.cssText =
        'position:fixed;bottom:16px;right:16px;z-index:99998;width:min(520px,46vw);max-height:42vh;background:#000;border-radius:10px;box-shadow:0 8px 32px rgba(0,0,0,.55);overflow:hidden;display:none;';
      document.body.appendChild(wrap);
    }
    var v = wrap.querySelector('video');
    if (!v) {
      v = document.createElement('video');
      v.setAttribute('playsinline', '');
      v.setAttribute('muted', '');
      v.setAttribute('controls', '');
      v.style.cssText = 'width:100%;height:auto;display:block;background:#000;';
      wrap.appendChild(v);
    }
    if (wrap._evoUrl !== url) {
      destroyHls(wrap);
      wrap._evoUrl = url;
      waitForManifest(url, function attach() {
        if (wrap._evoUrl !== url) return;
        if (typeof window.Hls !== 'undefined' && window.Hls.isSupported()) {
          // Sliding-window HLS from ffmpeg is not LL-HLS; lowLatencyMode causes aggressive stalls/rebuffers.
          var hls = new window.Hls({
            enableWorker: true,
            lowLatencyMode: false,
            maxBufferLength: 90,
            maxMaxBufferLength: 180,
            liveSyncDurationCount: 4,
            liveMaxLatencyDurationCount: Infinity,
          });
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
    wrap.style.display = 'block';
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
  }

  function onState(state) {
    if (!state) return;
    var u = state.videoStreamUrl;
    if (u && state.status && state.status !== 'stop') {
      ensureVideo(u);
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
