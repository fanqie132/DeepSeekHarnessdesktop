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

  // 边沿检测：监控弹窗/接管面板从无到有，出现时响一次
  // 覆盖：通用 modal(aria-modal)、审批(data-approval-key)、提问(data-question-key)、计划评审(data-plan-review-key)
  var active = false;
  function scan() {
    var modal = document.querySelector(
      '[aria-modal="true"], [data-approval-key], [data-question-key], [data-plan-review-key]'
    );
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
