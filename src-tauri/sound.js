(function () {
  if (window.__dshApproveSound) return;
  window.__dshApproveSound = true;

  // 审批/提问提示音：双音提醒
  function playApprove() {
    try {
      var ctx = new (window.AudioContext || window.webkitAudioContext)();
      var play = function (freq, when, dur) {
        var osc = ctx.createOscillator();
        var g = ctx.createGain();
        osc.type = "sine";
        osc.frequency.value = freq;
        g.gain.setValueAtTime(0.22, ctx.currentTime + when);
        g.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + when + dur);
        osc.connect(g);
        g.connect(ctx.destination);
        osc.start(ctx.currentTime + when);
        osc.stop(ctx.currentTime + when + dur + 0.05);
      };
      play(880, 0, 0.5);
      play(660, 0.15, 0.4);
    } catch (e) {}
  }

  // 边沿检测：监控 aria-modal 弹窗（审批/提问）从无到有，出现时响一次
  var active = false;
  function scan() {
    var modal = document.querySelector('[aria-modal="true"]');
    if (modal && !active) {
      active = true;
      playApprove();
    } else if (!modal) {
      active = false;
    }
  }
  try {
    if (document.body) {
      new MutationObserver(scan).observe(document.body, {
        childList: true,
        subtree: true,
      });
      scan();
    }
  } catch (e) {}
})();
