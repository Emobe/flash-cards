// Temporary demo cards for the card sandbox spike (step 0.6, ADR 0005). Removed with the spike
// panel once real card rendering exists. Card HTML is built here only for the demo.

const STYLE = `<style>
  body { font: 16px system-ui, sans-serif; margin: 0; padding: 16px; background: #fffbe8; color: #222; }
  button { min-height: 44px; font-size: 1rem; padding: 0 14px; margin: 4px 4px 4px 0; }
  img { display: block; margin: 8px 0; border: 2px solid #8a6d00; }
  audio { display: block; width: 100%; margin: 8px 0; }
  code { background: #0001; padding: 0 4px; }
</style>`;

/** A legitimate card: styled HTML, an image, audio and a little JavaScript. */
export function sampleCard(): string {
  return `<!doctype html><html><head><meta charset="utf-8">${STYLE}</head><body>
<h2>Cześć</h2>
<p>"Hello" in Polish. <code>sample.png</code> and <code>sample.wav</code> come from the core.</p>
<img id="pic" src="sample.png" width="64" height="64" alt="sample image">
<audio id="sound" src="sample.wav" controls></audio>
<button id="play" type="button">Play sound</button>
<button id="hint" type="button">Show hint</button>
<p id="hintText" hidden></p>
<p id="status">Script: not run</p>
<script>
(function () {
  var status = document.getElementById("status");
  var pic = document.getElementById("pic");
  var sound = document.getElementById("sound");
  var lines = { script: "Script ran, number " + Math.floor(Math.random() * 1000), image: "Image: loading", audio: "Audio: not played" };
  function render() { status.textContent = lines.script + " | " + lines.image + " | " + lines.audio; }
  function imageState() {
    lines.image = pic.complete && pic.naturalWidth > 0 ? "Image: loaded " + pic.naturalWidth + "x" + pic.naturalHeight : "Image: FAILED";
    render();
  }
  if (pic.complete) imageState(); else { pic.onload = imageState; pic.onerror = imageState; }
  sound.addEventListener("error", function () { lines.audio = "Audio: FAILED"; render(); });
  document.getElementById("play").addEventListener("click", function () {
    sound.play().then(function () { lines.audio = "Audio: playing"; render(); }, function (e) { lines.audio = "Audio: FAILED (" + e.name + ")"; render(); });
  });
  sound.addEventListener("ended", function () { lines.audio = "Audio: played to the end"; render(); });
  document.getElementById("hint").addEventListener("click", function () {
    var hint = document.getElementById("hintText");
    hint.hidden = false;
    hint.textContent = "Hint: it is a greeting. Random number " + Math.floor(Math.random() * 1000);
  });
  render();
})();
</script>
</body></html>`;
}

/**
 * A card that tries to break out. Every attempt reports "blocked", "SUCCEEDED" or something in
 * between, inside the card. `appOrigin` is the main page's origin, which a real card would not know
 * but an attacker can guess.
 */
export function maliciousCard(appOrigin: string): string {
  const origin = JSON.stringify(appOrigin).replace(/</g, "\\u003c");
  return `<!doctype html><html><head><meta charset="utf-8">${STYLE}
<style>
  h3 { margin: 0 0 4px; } ul { list-style: none; padding: 0; margin: 8px 0; }
  li { font-size: 0.8125rem; padding: 2px 0; border-bottom: 1px solid #0001; }
  .group { font-weight: bold; margin-top: 10px; border: 0; }
  .blocked { color: #146c2e; } .SUCCEEDED { color: #fff; background: #b3261e; font-weight: bold; }
  .other { color: #6a5300; }
</style></head><body>
<h3>Malicious card</h3>
<p id="summary">Running...</p>
<ul id="rows"></ul>
<script>
(function () {
  var APP = ${origin};
  var TIMEOUT = 2500;
  var GUESS = "AAAAAAAAAAAAAAAAAAAAAA";
  var tauri = window.__TAURI_INTERNALS__;

  var rows = document.getElementById("rows");
  var counts = { blocked: 0, succeeded: 0, other: 0 };
  var pending = 0;
  var violations = 0;
  document.addEventListener("securitypolicyviolation", function () { violations++; });

  var NO_BRIDGE = { text: "no bridge in frame" };
  var UNOBSERVABLE = { text: "attempted, outcome not visible from here (the app panel checks)" };
  function info(text) { return { text: text }; }

  var lastGroup = "";
  function addRow(group, name) {
    if (group !== lastGroup) {
      lastGroup = group;
      var g = document.createElement("li");
      g.className = "group";
      g.textContent = group;
      rows.appendChild(g);
    }
    var li = document.createElement("li");
    li.textContent = name + ": running...";
    rows.appendChild(li);
    return li;
  }

  function summarize() {
    document.getElementById("summary").textContent =
      counts.blocked + " blocked, " + counts.succeeded + " SUCCEEDED, " + counts.other +
      " other. CSP violations seen: " + violations + ".";
    document.documentElement.setAttribute("data-succeeded", String(counts.succeeded));
    document.documentElement.setAttribute("data-done", "1");
  }

  // fn returns true (the attack worked), false (it did not), an info object, or a promise of one.
  // A throw or a rejection counts as blocked.
  function attempt(group, name, fn) {
    var li = addRow(group, name);
    var finished = false;
    pending++;
    function finish(result, error) {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      var kind, text;
      if (error !== undefined) { kind = "blocked"; text = "blocked (" + (error && error.name ? error.name : String(error)) + ")"; }
      else if (result === true) { kind = "succeeded"; text = "SUCCEEDED"; }
      else if (result === false) { kind = "blocked"; text = "blocked"; }
      else if (result === undefined) { kind = "other"; text = "no result"; }
      else if (result === "timeout") { kind = "other"; text = "no answer within " + TIMEOUT / 1000 + " s (blocked, or not visible from here)"; }
      else { kind = "other"; text = result.text; }
      counts[kind]++;
      li.className = kind === "succeeded" ? "SUCCEEDED" : kind;
      li.textContent = name + ": " + text;
      pending--;
      if (pending === 0) summarize();
    }
    var timer = setTimeout(function () { finish("timeout"); }, TIMEOUT);
    try {
      Promise.resolve(fn()).then(function (r) { finish(r); }, function (e) { finish(undefined, e === undefined ? "rejected" : e); });
    } catch (e) {
      finish(undefined, e);
    }
  }

  function bridge(cmd, args) {
    if (!tauri || typeof tauri.invoke !== "function") return NO_BRIDGE;
    return tauri.invoke(cmd, args).then(function () { return true; });
  }
  function rawIpc(post) {
    if (!post) return NO_BRIDGE;
    post(JSON.stringify({ cmd: "call", callback: 1, error: 2, payload: { token: GUESS, method: "debugEmitEvent", input: { message: "FROM-CARD" } }, options: {} }));
    return UNOBSERVABLE;
  }
  function loaded(element, parent) {
    return new Promise(function (resolve) {
      element.onload = function () { resolve(true); };
      element.onerror = function () { resolve(false); };
      parent.appendChild(element);
    });
  }

  // 1. The bridge. On Android the frame has Tauri's invoke, so only the session token stops these.
  var CALL = { method: "debugEmitEvent", input: { message: "FROM-CARD" } };
  attempt("Bridge", "call, no token", function () { return bridge("call", CALL); });
  attempt("Bridge", "call, guessed token", function () { return bridge("call", { token: GUESS, method: CALL.method, input: CALL.input }); });
  attempt("Bridge", "handshake", function () { return bridge("handshake", {}); });
  attempt("Bridge", "subscribe, guessed token", function () { return bridge("subscribe", { token: GUESS, onNotice: "__CHANNEL__:0" }); });
  attempt("Bridge", "cancel, guessed token", function () { return bridge("cancel", { token: GUESS, op: 1 }); });
  attempt("Bridge", "plugin:event|emit", function () { return bridge("plugin:event|emit", { event: "fc-spike", payload: "FROM-CARD" }); });
  attempt("Bridge", "plugin:__TAURI_CHANNEL__|fetch", function () { return bridge("plugin:__TAURI_CHANNEL__|fetch", { id: 0 }); });
  attempt("Bridge", "window.ipc.postMessage", function () { return rawIpc(window.ipc && window.ipc.postMessage ? function (m) { window.ipc.postMessage(m); } : null); });
  attempt("Bridge", "webkit messageHandlers.ipc.postMessage", function () {
    var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc;
    return rawIpc(h ? function (m) { h.postMessage(m); } : null);
  });

  // 2. The parent, the top window and other cards.
  attempt("Parent and top", "read parent.document", function () { return typeof parent.document.title === "string"; });
  attempt("Parent and top", "read parent.location.href", function () { return typeof parent.location.href === "string"; });
  attempt("Parent and top", "read top.document", function () { return typeof top.document.title === "string"; });
  attempt("Parent and top", "read another frame's document (parent.frames[i])", function () {
    for (var i = 0; i < parent.frames.length; i++) {
      if (parent.frames[i] !== window) return typeof parent.frames[i].document.title === "string";
    }
    return info("no other frame to try");
  });
  attempt("Parent and top", "count frames (parent.frames.length)", function () { return info("browsers allow reading the number: " + parent.frames.length + " (no content)"); });
  attempt("Parent and top", "assign top.location", function () { top.location.href = "https://example.com/?from=card-top-location"; return UNOBSERVABLE; });
  attempt("Parent and top", "window.open", function () { return window.open("https://example.com/?from=card-open", "_blank") !== null; });
  attempt("Parent and top", "form with target=_top", function () {
    var f = document.createElement("form");
    f.method = "get"; f.action = APP + "/?from=card-form"; f.target = "_top";
    document.body.appendChild(f);
    f.submit();
    return UNOBSERVABLE;
  });
  attempt("Parent and top", "click a link with target=_top", function () {
    var a = document.createElement("a");
    a.href = APP + "/?from=card-link"; a.target = "_top"; a.textContent = "x";
    document.body.appendChild(a);
    a.click();
    return UNOBSERVABLE;
  });
  attempt("Parent and top", "BroadcastChannel to the app", function () { new BroadcastChannel("fc-spike").postMessage("FROM-CARD"); return UNOBSERVABLE; });
  attempt("Parent and top", "set document.domain", function () { document.domain = location.hostname || "localhost"; return true; });
  attempt("Parent and top", "huge height and unknown messages to the parent", function () {
    parent.postMessage({ type: "height", px: 1e9 }, "*");
    parent.postMessage({ type: "height", px: -5 }, "*");
    parent.postMessage({ type: "navigate", url: "https://example.com/" }, "*");
    parent.postMessage({ type: "ready" }, "*");
    parent.postMessage("height", "*");
    setTimeout(function () { parent.postMessage({ type: "height", px: Math.ceil(document.documentElement.scrollHeight) }, "*"); }, 800);
    return UNOBSERVABLE;
  });
  attempt("Parent and top", "nested iframe to the app", function () {
    var f = document.createElement("iframe");
    f.src = APP + "/?from=card-iframe";
    return loaded(f, document.body);
  });

  // 3. Storage.
  attempt("Storage", "localStorage", function () { localStorage.setItem("fc", "1"); return true; });
  attempt("Storage", "sessionStorage", function () { sessionStorage.setItem("fc", "1"); return true; });
  attempt("Storage", "indexedDB", function () {
    return new Promise(function (resolve) {
      var r = indexedDB.open("fc-card");
      r.onsuccess = function () { resolve(true); };
      r.onerror = function () { resolve(false); };
    });
  });
  attempt("Storage", "Cache API", function () { return typeof caches === "undefined" ? false : caches.open("fc").then(function () { return true; }); });
  attempt("Storage", "document.cookie", function () { document.cookie = "fc=1"; return document.cookie.indexOf("fc=1") !== -1; });
  attempt("Storage", "OPFS (navigator.storage.getDirectory)", function () { return navigator.storage.getDirectory().then(function () { return true; }); });
  attempt("Storage", "service worker", function () { return navigator.serviceWorker.register("/sw.js").then(function () { return true; }); });

  // 4. Network. CSP has no connect-src, and default-src is 'none'.
  var targets = [["the app origin", APP + "/"], ["ipc://localhost", "ipc://localhost/call"], ["http://ipc.localhost", "http://ipc.localhost/call"], ["https://example.com", "https://example.com/?from=card"]];
  targets.forEach(function (t) {
    attempt("Network", "fetch " + t[0], function () { return fetch(t[1]).then(function () { return true; }); });
    attempt("Network", "XMLHttpRequest " + t[0], function () {
      return new Promise(function (resolve) {
        var x = new XMLHttpRequest();
        x.open("GET", t[1]);
        x.onload = function () { resolve(true); };
        x.onerror = function () { resolve(false); };
        x.send();
      });
    });
  });
  attempt("Network", "WebSocket wss://example.com", function () {
    return new Promise(function (resolve) {
      var s = new WebSocket("wss://example.com/");
      s.onopen = function () { resolve(true); };
      s.onerror = function () { resolve(false); };
      s.onclose = function () { resolve(false); };
    });
  });
  attempt("Network", "navigator.sendBeacon", function () { return navigator.sendBeacon("https://example.com/?from=card-beacon", "x"); });
  [["the app origin", APP + "/favicon.ico"], ["https://example.com", "https://example.com/x.png"]].forEach(function (t) {
    attempt("Network", "<img> from " + t[0], function () { var e = new Image(); e.src = t[1]; return loaded(e, document.body); });
  });
  [["the app origin", APP + "/"], ["https://example.com", "https://example.com/x.css"]].forEach(function (t) {
    attempt("Network", "<link rel=stylesheet> from " + t[0], function () { var e = document.createElement("link"); e.rel = "stylesheet"; e.href = t[1]; return loaded(e, document.head); });
  });
  [["the app origin", APP + "/"], ["https://example.com", "https://example.com/x.js"]].forEach(function (t) {
    attempt("Network", "<script src> from " + t[0], function () { var e = document.createElement("script"); e.src = t[1]; return loaded(e, document.head); });
  });
  attempt("Network", "import() of a data: module", function () { return import("data:text/javascript,export default 1").then(function () { return true; }); });
  attempt("Network", "Worker from a blob", function () {
    return new Promise(function (resolve) {
      var w = new Worker(URL.createObjectURL(new Blob(["postMessage(1)"])));
      w.onmessage = function () { resolve(true); };
      w.onerror = function () { resolve(false); };
    });
  });

  // 5. Prompts and permissions.
  attempt("Prompts and permissions", "alert", function () { alert("card alert"); return info("attempted (no dialog should have appeared)"); });
  attempt("Prompts and permissions", "confirm", function () { return confirm("card confirm") === true; });
  attempt("Prompts and permissions", "requestFullscreen", function () { return document.documentElement.requestFullscreen().then(function () { return true; }); });
  attempt("Prompts and permissions", "geolocation", function () {
    return new Promise(function (resolve) { navigator.geolocation.getCurrentPosition(function () { resolve(true); }, function () { resolve(false); }); });
  });
  attempt("Prompts and permissions", "getUserMedia (microphone)", function () { return navigator.mediaDevices.getUserMedia({ audio: true }).then(function () { return true; }); });
  attempt("Prompts and permissions", "clipboard.readText", function () { return navigator.clipboard.readText().then(function () { return true; }); });
  attempt("Prompts and permissions", "Notification.requestPermission", function () { return Notification.requestPermission().then(function (p) { return p === "granted"; }); });
})();
</script>
</body></html>`;
}

/** A card that navigates its own frame after one second. The app must remove it. */
export function navigatingCard(url: string): string {
  const target = JSON.stringify(url).replace(/</g, "\\u003c");
  return `<!doctype html><html><head><meta charset="utf-8">${STYLE}</head><body>
<h3>Navigating card</h3>
<p>This card navigates its own frame in one second.</p>
<script>setTimeout(function () { location.href = ${target}; }, 1000);</script>
</body></html>`;
}
