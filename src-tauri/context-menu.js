(function () {
  if (window.__dshCtxMenu) return;
  window.__dshCtxMenu = true;
  var menu = document.createElement('div');
  menu.id = '__dsh-ctxmenu';
  menu.style.cssText = 'position:fixed;z-index:2147483647;display:none;min-width:150px;background:#fff;border:1px solid rgba(0,0,0,.1);border-radius:8px;box-shadow:0 4px 16px rgba(0,0,0,.15);padding:4px;font-family:system-ui,sans-serif;font-size:13px;color:#1f2937;user-select:none;';
  document.documentElement.appendChild(menu);
  var dark = window.matchMedia('(prefers-color-scheme: dark)');
  function applyTheme() {
    if (dark.matches) {
      menu.style.background = '#1f2937'; menu.style.color = '#f3f4f6'; menu.style.borderColor = 'rgba(255,255,255,.12)';
    } else {
      menu.style.background = '#ffffff'; menu.style.color = '#1f2937'; menu.style.borderColor = 'rgba(0,0,0,.1)';
    }
  }
  dark.addEventListener('change', applyTheme); applyTheme();
  function hide() { menu.style.display = 'none'; }
  function show(x, y, items) {
    menu.innerHTML = '';
    items.forEach(function (it) {
      var item = document.createElement('div');
      item.textContent = it.label;
      item.style.cssText = 'padding:7px 12px;border-radius:6px;cursor:pointer;';
      item.addEventListener('mouseenter', function () { item.style.background = dark.matches ? 'rgba(255,255,255,.1)' : '#f3f4f6'; });
      item.addEventListener('mouseleave', function () { item.style.background = 'transparent'; });
      item.addEventListener('click', function () { hide(); if (it.action) it.action(); });
      menu.appendChild(item);
    });
    var r = menu.getBoundingClientRect();
    menu.style.left = Math.min(x, window.innerWidth - r.width - 4) + 'px';
    menu.style.top = Math.min(y, window.innerHeight - r.height - 4) + 'px';
    menu.style.display = 'block';
  }
  document.addEventListener('click', hide);
  document.addEventListener('scroll', hide, true);
  // 可复制文本的错误提示层（替代 alert，便于排查/反馈）
  function showError(text) {
    var box = document.createElement('div');
    box.style.cssText = 'position:fixed;z-index:2147483647;left:50%;top:50%;transform:translate(-50%,-50%);max-width:80%;max-height:70%;overflow:auto;background:#fff;border:1px solid #e5e7eb;border-radius:10px;box-shadow:0 8px 30px rgba(0,0,0,.2);padding:16px 20px;font-family:system-ui,sans-serif;font-size:13px;color:#1f2937;text-align:left;user-select:text;';
    var title = document.createElement('div');
    title.textContent = 'DeepSeek Harness';
    title.style.cssText = 'font-weight:600;margin-bottom:8px;color:#111827;';
    var msg = document.createElement('pre');
    msg.textContent = text;
    msg.style.cssText = 'white-space:pre-wrap;word-break:break-all;margin:0 0 12px;font-family:inherit;';
    var btn = document.createElement('button');
    btn.textContent = '确定';
    btn.style.cssText = 'background:#4176e6;color:#fff;border:none;border-radius:6px;padding:6px 16px;cursor:pointer;float:right;';
    btn.onclick = function () { box.remove(); };
    box.appendChild(title); box.appendChild(msg); box.appendChild(btn);
    document.body.appendChild(box);
    setTimeout(function () { msg.select(); }, 50);
  }
  document.addEventListener('contextmenu', function (e) {
    var img = e.target.closest ? e.target.closest('img') : null;
    if (img) {
      e.preventDefault();
      var url = img.currentSrc || img.src;
      show(e.clientX, e.clientY, [
        { label: '复制图片', action: function () {
            fetch(url).then(function (r) { return r.blob(); }).then(function (b) {
              var t = b.type && b.type.indexOf('image/') === 0 ? b.type : 'image/png';
              try { navigator.clipboard.write([new ClipboardItem({ [t]: b })]); } catch (err) {}
            }).catch(function () {});
          } },
        { label: '复制图片链接', action: function () { navigator.clipboard.writeText(url); } },
        { label: '图片另存为', action: function () {
            if (!(window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)) {
              showError('Tauri 环境不可用');
              return;
            }
            var raw = img.currentSrc || img.src || '';
            var abs;
            try { abs = new URL(raw, location.href).href; } catch (e) { abs = raw; }
            var invoke = window.__TAURI__.core.invoke;
            if (abs.indexOf('data:') === 0) {
              // data URL：数据已在 URL 内，直接交给 Rust 解码保存
              invoke('save_image_data', { data: abs, filename: 'image' })
                .then(function () {}).catch(function (e) { showError('另存为失败: ' + e); });
            } else if (abs.indexOf('blob:') === 0) {
              // blob URL：仅在页面内有效，先 fetch 转 base64 再交给 Rust
              fetch(abs).then(function (r) { return r.blob(); }).then(function (blob) {
                var reader = new FileReader();
                reader.onload = function () {
                  invoke('save_image_data', { data: reader.result, filename: 'image' })
                    .then(function () {}).catch(function (e) { showError('另存为失败: ' + e); });
                };
                reader.readAsDataURL(blob);
              }).catch(function (e) { showError('另存为失败: ' + e); });
            } else {
              // http(s) URL：Rust 直接下载保存
              invoke('save_image', { url: abs })
                .then(function () {}).catch(function (e) { showError('另存为失败: ' + e); });
            }
          } },
      ]);
      return;
    }
    // 输入框/可编辑区域：复制 / 粘贴 / 全选
    var editable = e.target.closest ? e.target.closest('input, textarea, [contenteditable="true"]') : null;
    if (editable) {
      e.preventDefault();
      show(e.clientX, e.clientY, [
        { label: '复制', action: function () { document.execCommand('copy'); } },
        { label: '粘贴', action: function () {
            // 现代 WebView 已禁用 execCommand('paste')，回退到剪贴板 API + 手动插入
            if (document.execCommand('paste')) return;
            navigator.clipboard.readText().then(function (t) {
              if (!t) return;
              var el = document.activeElement;
              if (el && (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT' || el.isContentEditable)) {
                if (typeof el.selectionStart === 'number') {
                  var s = el.selectionStart, e2 = el.selectionEnd;
                  el.value = el.value.slice(0, s) + t + el.value.slice(e2);
                  el.selectionStart = el.selectionEnd = s + t.length;
                } else {
                  document.execCommand('insertText', false, t);
                }
              } else {
                document.execCommand('insertText', false, t);
              }
            }).catch(function () {});
          } },
        { label: '全选', action: function () { document.execCommand('selectAll'); } },
      ]);
      return;
    }
    // 有选中文本：复制 / 全选
    if (window.getSelection && window.getSelection().toString().length > 0) {
      e.preventDefault();
      show(e.clientX, e.clientY, [
        { label: '复制', action: function () { document.execCommand('copy'); } },
        { label: '全选', action: function () { document.execCommand('selectAll'); } },
      ]);
      return;
    }
    // 空白/普通区域：阻止默认菜单，不弹任何菜单
    e.preventDefault();
  });
})();
