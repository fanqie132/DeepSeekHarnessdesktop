(function () {
  if (window.__dshSound) return;
  window.__dshSound = true;

  // 单例 Web Audio 上下文，首次用户交互预热
  var ctx = null;
  var comp = null;
  function audio() {
    if (!ctx) {
      try {
        ctx = new (window.AudioContext || window.webkitAudioContext)();
        // 软限幅器：多音叠加时压住峰值，防削波失真（允许增益提到 1.0）
        comp = ctx.createDynamicsCompressor();
        comp.threshold.value = -3;
        comp.knee.value = 0;
        comp.ratio.value = 20;
        comp.attack.value = 0.001;
        comp.release.value = 0.1;
        comp.connect(ctx.destination);
      } catch (e) {}
    }
    return ctx;
  }
  // 预热（页面加载 + 用户交互）
  function warm() {
    var c = audio();
    if (c && c.state === "suspended") c.resume().catch(function () {});
  }
  warm();
  ["pointerdown", "keydown", "touchstart"].forEach(function (ev) {
    window.addEventListener(ev, warm, { once: false, passive: true });
  });

  function tone(freq, when, dur) {
    var c = audio();
    if (!c) return;
    var osc = c.createOscillator();
    var g = c.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    g.gain.setValueAtTime(1.0, c.currentTime + when);
    g.gain.exponentialRampToValueAtTime(0.001, c.currentTime + when + dur);
    osc.connect(g);
    g.connect(comp || c.destination);
    osc.start(c.currentTime + when);
    osc.stop(c.currentTime + when + dur + 0.05);
  }
  // 弹窗（审批/提问/计划评审）：双音提醒
  function playApprove() {
    tone(880, 0, 0.5);
    tone(660, 0.15, 0.4);
  }
  // 完成（每轮回复结束 / goal 完成）：上行三音
  function playComplete() {
    tone(523, 0, 0.15);
    tone(659, 0.15, 0.15);
    tone(784, 0.3, 0.4);
  }

  // ---- 弹窗边沿检测（审批/提问/计划评审，不含 aria-modal 设置弹窗）----
  var active = false;
  function scanModal() {
    var modal = document.querySelector(
      "[data-approval-key], [data-question-key], [data-plan-review-key]"
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
      new MutationObserver(scanModal).observe(document.body, {
        childList: true,
        subtree: true,
      });
      scanModal();
    }
  } catch (e) {}

  // ---- 完成检测：轮询 session.list，running 翻转 + goal 完成 ----
  // 频率 3s（平衡提示及时性与常驻开销）；localStorage 设 dshSound=off 可整体关闭
  var pollEnabled = localStorage.getItem("dshSound") !== "off";
  var runningState = {};
  var goalPhase = {};
  var completedGoal = {};
  var lastCompleteAt = {};
  function fetchSessions() {
    return fetch("/api/session.list", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        type: "client-request",
        rpcId: "snd" + Date.now(),
        method: "session.list",
        payload: {},
      }),
    })
      .then(function (r) { return r.json(); })
      .catch(function () { return null; });
  }
  function onSessions(json) {
    var items = (json && json.result && json.result.value && json.result.value.items) || [];
    var now = Date.now();
    items.forEach(function (s) {
      var id = s.sessionId;
      var running = !!s.running;
      // 每轮回复结束：running true→false，30s 节流
      var prevRunning = runningState[id];
      if (!running && prevRunning === true) {
        if (!lastCompleteAt[id] || now - lastCompleteAt[id] > 30000) {
          lastCompleteAt[id] = now;
          playComplete();
        }
      }
      runningState[id] = running;
      // goal 完成：phase active→complete，每 sessionId 一次
      var phase = (s.projections && s.projections.values && s.projections.values.goal && s.projections.values.goal.phase) || "";
      var prevPhase = goalPhase[id];
      if (phase === "complete" && prevPhase === "active" && !completedGoal[id]) {
        completedGoal[id] = true;
        playComplete();
      }
      goalPhase[id] = phase;
    });
  }
  if (pollEnabled) {
    setInterval(function () {
      fetchSessions().then(onSessions);
    }, 3000);
  }
})();
