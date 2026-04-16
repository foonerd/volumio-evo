(function () {
  var MAX_BACKWARD_FIX_MS = 2200;
  var BACKWARD_THRESHOLD_MS = 450;

  function startPatch() {
    var el = document.body;
    if (!el) return false;
    var inj = angular.element(el).injector();
    if (!inj) return false;

    var $rootScope = inj.get('$rootScope');
    var $interval = inj.get('$interval');
    var peakElapsed = 0;

    $interval(function () {
      try {
        var ps = inj.get('playerService');
        if (ps && ps.state && ps.state.status === 'play' && typeof ps.elapsedTime === 'number') {
          if (ps.elapsedTime > peakElapsed) peakElapsed = ps.elapsedTime;
        }
      } catch (_e) {}
    }, 250);

    var lastUri = null;
    $rootScope.$on('socket:pushState', function (_ev, state) {
      try {
        var ps = inj.get('playerService');
        if (!state || !ps) return;

        if (state.uri !== lastUri) {
          lastUri = state.uri;
          peakElapsed = 0;
        }

        if (state.status === 'play' && typeof ps.elapsedTime === 'number') {
          if (
            peakElapsed > 0 &&
            ps.elapsedTime + BACKWARD_THRESHOLD_MS < peakElapsed &&
            peakElapsed - ps.elapsedTime < MAX_BACKWARD_FIX_MS
          ) {
            ps.elapsedTime = peakElapsed;
            ps.updateSeek();
          }
        }

        if (state.status === 'play' && typeof ps.elapsedTime === 'number') {
          peakElapsed = ps.elapsedTime;
        } else {
          peakElapsed = 0;
        }
      } catch (_e) {}
    });

    return true;
  }

  function tryStart() {
    if (startPatch()) return;
    var tries = 0;
    var id = setInterval(function () {
      tries += 1;
      if (startPatch() || tries > 120) clearInterval(id);
    }, 50);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', tryStart);
  } else {
    tryStart();
  }
})();
