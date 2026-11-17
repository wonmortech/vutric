// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// The Halqa effect, drawn with Canvas 2D from the same timeline as crates/vutric-overlay/src/timeline.rs
// (travel 1.05 s, hold 1.4 s, dock 0.45 s; back-ease opening, cubic Bezier flight, 3.2 Hz spring landing),
// plus the small interactive demos of the design-system page.
"use strict";

(function () {
  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------- easing ----------
  function clamp01(x) { return x < 0 ? 0 : x > 1 ? 1 : x; }
  function easeOutCubic(x) { var k = 1 - x; return 1 - k * k * k; }
  function easeInCubic(x) { return x * x * x; }
  function easeBackOut(x) { var c1 = 1.6, c3 = c1 + 1; var k = x - 1; return 1 + c3 * k * k * k + c1 * k * k; }
  function spring(t) {
    // underdamped: 3.2 Hz, damping ratio 0.42 (docs/effects.md)
    var w = 2 * Math.PI * 3.2, z = 0.42, wd = w * Math.sqrt(1 - z * z);
    return Math.exp(-z * w * t) * Math.sin(wd * t);
  }
  function bezier(p0, p1, p2, p3, t) {
    var u = 1 - t, a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t;
    return [a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]];
  }

  var CFG = { travel: 1.05, hold: 1.4, dock: 0.45, gap: 0.9 };
  var TOTAL = CFG.travel + CFG.hold + CFG.dock + CFG.gap;
  // Under reduced motion the effect rests on one frame: portal open, photo halfway out with its trail.
  var STILL = 0.42;

  // ---------- the photo on the card: a dusk landscape, drawn once ----------
  function makePhoto() {
    var c = document.createElement("canvas");
    c.width = 480; c.height = 360;
    var g = c.getContext("2d");
    var sky = g.createLinearGradient(0, 0, 0, 360);
    sky.addColorStop(0, "#22336f"); sky.addColorStop(0.45, "#7a5a8a"); sky.addColorStop(0.62, "#e08a62"); sky.addColorStop(1, "#f6c98a");
    g.fillStyle = sky; g.fillRect(0, 0, 480, 360);
    var sun = g.createRadialGradient(330, 205, 4, 330, 205, 120);
    sun.addColorStop(0, "rgba(255,240,200,1)"); sun.addColorStop(0.18, "rgba(255,214,150,0.9)"); sun.addColorStop(1, "rgba(255,190,120,0)");
    g.fillStyle = sun; g.fillRect(0, 0, 480, 360);
    var hills = ["#5a3f68", "#3a2d58", "#241f45", "#151634"];
    for (var i = 0; i < hills.length; i++) {
      g.fillStyle = hills[i];
      g.beginPath(); g.moveTo(0, 360);
      for (var x = 0; x <= 480; x += 8) {
        var y = 360 * (0.58 + 0.09 * i) + 13 * Math.sin(x * (0.012 + 0.006 * i) + i * 1.7) + 6 * Math.cos(x * 0.045 + i);
        g.lineTo(x, y);
      }
      g.lineTo(480, 360); g.closePath(); g.fill();
    }
    return c;
  }

  function roundRect(ctx, x, y, w, h, r) {
    ctx.beginPath();
    if (ctx.roundRect) { ctx.roundRect(x, y, w, h, r); return; }
    ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
  }

  // ---------- one effect instance on one canvas ----------
  function Halqa(canvas, opts) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.opts = opts;
    this.photo = makePhoto();
    this.t = opts.start || 0;
    this.playing = !reduceMotion;
    this.visible = true;
    this.last = 0;
    this.raf = 0;
    this.w = 0; this.h = 0;
    this.onTime = null;
    var self = this;
    this.frame = function (now) { self.tick(now); };
    if ("ResizeObserver" in window) {
      new ResizeObserver(function () { self.resize(); self.draw(); }).observe(canvas);
    }
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (entries) {
        self.visible = entries[0].isIntersecting;
        if (self.visible) self.kick();
      }, { threshold: 0.05 }).observe(canvas);
    }
    document.addEventListener("visibilitychange", function () { if (!document.hidden) self.kick(); });
    this.resize();
    this.draw();
    this.kick();
  }

  Halqa.prototype.resize = function () {
    var dpr = Math.min(window.devicePixelRatio || 1, 2);
    var rect = this.canvas.getBoundingClientRect();
    this.w = Math.max(1, rect.width); this.h = Math.max(1, rect.height);
    this.canvas.width = Math.round(this.w * dpr); this.canvas.height = Math.round(this.h * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  };

  Halqa.prototype.kick = function () {
    if (this.playing && this.visible && !document.hidden && !this.raf) {
      this.last = 0;
      this.raf = requestAnimationFrame(this.frame);
    }
  };

  Halqa.prototype.tick = function (now) {
    this.raf = 0;
    if (!this.playing || !this.visible || document.hidden) return;
    var dt = this.last ? Math.min((now - this.last) / 1000, 0.05) : 0;
    this.last = now;
    this.t = (this.t + dt) % TOTAL;
    this.draw();
    if (this.onTime) this.onTime(this.t);
    this.raf = requestAnimationFrame(this.frame);
  };

  Halqa.prototype.setPlaying = function (on) {
    this.playing = on;
    if (on) this.kick();
  };

  Halqa.prototype.layout = function () {
    var w = this.w, h = this.h, o = this.opts;
    var ry = Math.min(h * 0.34, 320), rx = ry * 0.42;
    var cardH = Math.min(h * 0.36, w * 0.3, 300), cardW = cardH * 4 / 3;
    return {
      ring: [w - rx * 0.55, h * o.ringY],
      rx: rx, ry: ry,
      land: [w * o.landX, h * o.landY],
      dock: [w - 36, h - 36],
      cardW: cardW, cardH: cardH
    };
  };

  // Sample the timeline at time t: everything the draw step needs, nothing drawn yet.
  Halqa.prototype.sample = function (t) {
    var L = this.layout(), travel = CFG.travel;
    var u = t / travel;
    var open = clamp01(u / 0.24), close = clamp01((u - 0.6) / 0.3);
    var ring = {
      scale: easeBackOut(open) * (1 - 0.35 * easeInCubic(close)),
      alpha: clamp01(open * 3) * (1 - close)
    };
    var f = clamp01((u - 0.10) / 0.52), e = easeOutCubic(f);
    var p0 = L.ring, p3 = L.land;
    var dx = p0[0] - p3[0];
    var p1 = [p0[0] - dx * 0.25, p0[1] - L.ry * 0.55];
    var p2 = [p3[0] + dx * 0.2, p3[1] - L.ry * 0.12];
    var pos = bezier(p0, p1, p2, p3, e);
    var scale = 0.16 + 0.84 * e, rot = -0.42 * (1 - e), alpha = clamp01(f * 5);
    var tl = 0.62 * travel;
    if (t > tl) {
      var s = spring(t - tl);
      scale = 1 + 0.07 * s;
      rot = -0.05 * s;
    }
    var trail = [];
    if (f > 0 && f < 1) {
      for (var k = 1; k <= 4; k++) {
        var ek = easeOutCubic(clamp01(f - k * 0.075));
        if (ek <= 0) continue;
        trail.push({ pos: bezier(p0, p1, p2, p3, ek), scale: 0.16 + 0.84 * ek, rot: -0.42 * (1 - ek), alpha: (1 - f) * 0.55 / k });
      }
    }
    var dockT = t - travel - CFG.hold;
    if (dockT > 0) {
      var d = easeInCubic(clamp01(dockT / CFG.dock));
      pos = [pos[0] + (L.dock[0] - pos[0]) * d, pos[1] + (L.dock[1] - pos[1]) * d];
      scale *= 1 - 0.78 * d;
      alpha *= 1 - d;
    }
    return { L: L, ring: ring, card: { pos: pos, scale: scale, rot: rot, alpha: alpha }, trail: trail };
  };

  Halqa.prototype.draw = function () {
    var ctx = this.ctx, w = this.w, h = this.h;
    var S = this.sample(this.t), L = S.L;
    ctx.clearRect(0, 0, w, h);

    // portal
    if (S.ring.alpha > 0.002) {
      var r = L.ry * S.ring.scale, time = this.t;
      ctx.save();
      ctx.translate(L.ring[0], L.ring[1]);
      ctx.scale(L.rx / L.ry, 1);
      ctx.globalAlpha = S.ring.alpha;
      var lens = ctx.createRadialGradient(0, 0, 0, 0, 0, r);
      lens.addColorStop(0, "rgba(2,6,22,0.96)");
      lens.addColorStop(0.75, "rgba(8,20,64,0.9)");
      lens.addColorStop(1, "rgba(47,107,255,0.35)");
      ctx.fillStyle = lens;
      ctx.beginPath(); ctx.arc(0, 0, r, 0, Math.PI * 2); ctx.fill();
      ctx.lineWidth = 1.2;
      for (var i = 0; i < 4; i++) {
        var rr = r * ((0.25 + 0.2 * i + time * 0.22) % 0.95);
        ctx.strokeStyle = "rgba(120,160,255," + (0.12 * (1 - rr / r)).toFixed(3) + ")";
        ctx.beginPath(); ctx.arc(0, 0, rr, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.globalCompositeOperation = "lighter";
      var rim = ctx.createLinearGradient(0, -r, 0, r);
      rim.addColorStop(0, "#2f6bff"); rim.addColorStop(0.5, "#f4d9a6"); rim.addColorStop(1, "#2f6bff");
      var passes = [[46, 0.06], [26, 0.12], [13, 0.26], [6, 0.55], [2.4, 1]];
      for (var p = 0; p < passes.length; p++) {
        ctx.lineWidth = passes[p][0];
        ctx.globalAlpha = S.ring.alpha * passes[p][1];
        ctx.strokeStyle = p === passes.length - 1 ? "#fff3d6" : rim;
        ctx.beginPath(); ctx.arc(0, 0, r, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.restore();
    }

    // trail: contour echoes of the card
    for (var k = 0; k < S.trail.length; k++) {
      var tr = S.trail[k];
      ctx.save();
      ctx.translate(tr.pos[0], tr.pos[1]); ctx.rotate(tr.rot); ctx.scale(tr.scale, tr.scale);
      ctx.globalAlpha = tr.alpha;
      ctx.strokeStyle = k < 2 ? "#6f8ff0" : "#f4d9a6";
      ctx.lineWidth = 2 / tr.scale;
      roundRect(ctx, -L.cardW / 2, -L.cardH / 2, L.cardW, L.cardH, 16);
      ctx.stroke();
      ctx.restore();
    }

    // the card
    var c = S.card;
    if (c.alpha > 0.002) {
      ctx.save();
      ctx.translate(c.pos[0], c.pos[1]); ctx.rotate(c.rot); ctx.scale(c.scale, c.scale);
      ctx.globalAlpha = c.alpha;
      ctx.shadowColor = "rgba(0,0,0,0.55)"; ctx.shadowBlur = 42; ctx.shadowOffsetY = 22;
      roundRect(ctx, -L.cardW / 2, -L.cardH / 2, L.cardW, L.cardH, 16);
      ctx.fillStyle = "#151634"; ctx.fill();
      ctx.shadowColor = "transparent";
      ctx.save(); ctx.clip();
      ctx.drawImage(this.photo, -L.cardW / 2, -L.cardH / 2, L.cardW, L.cardH);
      var sheen = ctx.createLinearGradient(-L.cardW / 2, -L.cardH / 2, L.cardW / 2, L.cardH / 2);
      sheen.addColorStop(0, "rgba(255,255,255,0.16)"); sheen.addColorStop(0.45, "rgba(255,255,255,0)");
      ctx.fillStyle = sheen; ctx.fillRect(-L.cardW / 2, -L.cardH / 2, L.cardW, L.cardH);
      ctx.restore();
      ctx.lineWidth = 1.5 / c.scale; ctx.strokeStyle = "rgba(244,217,166,0.55)";
      roundRect(ctx, -L.cardW / 2, -L.cardH / 2, L.cardW, L.cardH, 16); ctx.stroke();
      ctx.restore();
    }
  };

  // ---------- mount ----------
  var canvasOk = (function () { try { return !!document.createElement("canvas").getContext("2d"); } catch (e) { return false; } })();
  var effects = {};
  if (!canvasOk) document.documentElement.classList.add("no-canvas");

  var heroCanvas = document.querySelector('[data-halqa="hero"]');
  if (canvasOk && heroCanvas) {
    var narrow = window.innerWidth < 768;
    effects.hero = new Halqa(heroCanvas, { ringY: narrow ? 0.2 : 0.42, landX: narrow ? 0.5 : 0.68, landY: narrow ? 0.19 : 0.4, start: reduceMotion ? STILL : 0 });
  }
  var studioCanvas = document.querySelector('[data-halqa="studio"]');
  var scrub = document.querySelector('[data-halqa-scrub="studio"]');
  var phases = Array.prototype.slice.call(document.querySelectorAll(".phases li"));
  function mark(t) {
    var name = t < 0.105 ? "open" : t < 0.65 ? "fly" : t < CFG.travel ? "land" : t < CFG.travel + CFG.hold ? "hold" : t < CFG.travel + CFG.hold + CFG.dock ? "dock" : "";
    phases.forEach(function (li) { li.classList.toggle("is-now", li.getAttribute("data-phase") === name); });
    if (scrub && document.activeElement !== scrub) scrub.value = String(Math.round((t / TOTAL) * 1000));
  }
  if (canvasOk && studioCanvas) {
    effects.studio = new Halqa(studioCanvas, { ringY: 0.5, landX: 0.42, landY: 0.5, start: STILL });
    effects.studio.onTime = mark;
    mark(effects.studio.t);
    if (scrub) {
      scrub.addEventListener("input", function () {
        var fx = effects.studio;
        fx.setPlaying(false);
        syncToggle("studio");
        fx.t = (Number(scrub.value) / 1000) * TOTAL;
        fx.draw(); mark(fx.t);
      });
    }
  }

  function syncToggle(name) {
    var btn = document.querySelector('[data-halqa-toggle="' + name + '"]');
    var fx = effects[name];
    if (!btn || !fx) return;
    btn.setAttribute("aria-pressed", String(!fx.playing));
    var hero = name === "hero";
    btn.textContent = fx.playing ? (hero ? "Pause animation" : "Pause") : (hero ? "Play animation" : "Play");
  }
  Array.prototype.forEach.call(document.querySelectorAll("[data-halqa-toggle]"), function (btn) {
    var name = btn.getAttribute("data-halqa-toggle");
    if (!effects[name]) { btn.hidden = true; return; }
    syncToggle(name);
    btn.addEventListener("click", function () {
      effects[name].setPlaying(!effects[name].playing);
      syncToggle(name);
    });
  });

  // ---------- theme switch ----------
  var root = document.documentElement;
  function currentTheme() { return root.getAttribute("data-theme") || "auto"; }
  function paintTheme() {
    Array.prototype.forEach.call(document.querySelectorAll("[data-theme-set]"), function (b) {
      b.setAttribute("aria-checked", String(b.getAttribute("data-theme-set") === currentTheme()));
    });
  }
  Array.prototype.forEach.call(document.querySelectorAll("[data-theme-set]"), function (b) {
    b.addEventListener("click", function () {
      var v = b.getAttribute("data-theme-set");
      if (v === "auto") root.removeAttribute("data-theme"); else root.setAttribute("data-theme", v);
      try { if (v === "auto") window.localStorage.removeItem("vutric-theme"); else window.localStorage.setItem("vutric-theme", v); } catch (e) { /* storage blocked: the choice lasts this visit */ }
      paintTheme();
    });
  });
  paintTheme();

  // ---------- radio groups built from buttons (send zone, effect picker) ----------
  Array.prototype.forEach.call(document.querySelectorAll('[role="radiogroup"]:not(.theme)'), function (group) {
    var radios = Array.prototype.slice.call(group.querySelectorAll('button[role="radio"]'));
    function select(i, focus) {
      radios.forEach(function (r, j) { r.setAttribute("aria-checked", String(i === j)); r.tabIndex = i === j ? 0 : -1; });
      if (focus) radios[i].focus();
    }
    radios.forEach(function (r, i) {
      r.tabIndex = r.getAttribute("aria-checked") === "true" ? 0 : -1;
      r.addEventListener("click", function () { select(i); });
      r.addEventListener("keydown", function (ev) {
        var step = ev.key === "ArrowRight" || ev.key === "ArrowDown" ? 1 : ev.key === "ArrowLeft" || ev.key === "ArrowUp" ? -1 : 0;
        if (!step) return;
        ev.preventDefault();
        select((i + step + radios.length) % radios.length, true);
      });
    });
  });

  // ---------- duration slider ----------
  var slider = document.querySelector("[data-slider]"), out = document.querySelector("[data-slider-out]");
  if (slider && out) {
    var show = function () { out.textContent = Number(slider.value).toFixed(1).replace(".", ",") + " s"; };
    slider.addEventListener("input", show); show();
  }

  // ---------- decorative QR-like patterns (not codes: they decode to nothing) ----------
  var SVG_NS = "http://www.w3.org/2000/svg";
  function qrSvg(seed) {
    var n = 25, s = seed >>> 0, d = "";
    function rnd() { s ^= s << 13; s >>>= 0; s ^= s >>> 17; s ^= s << 5; s >>>= 0; return s / 4294967296; }
    function inFinder(x, y) { return (x < 7 && y < 7) || (x > n - 8 && y < 7) || (x < 7 && y > n - 8); }
    function nearFinder(x, y) { return (x < 8 && y < 8) || (x > n - 9 && y < 8) || (x < 8 && y > n - 9); }
    for (var y = 0; y < n; y++) {
      for (var x = 0; x < n; x++) {
        var on;
        if (inFinder(x, y)) {
          var fx = x < 7 ? x : n - 1 - x, fy = y < 7 ? y : n - 1 - y;
          var ring = Math.max(Math.abs(fx - 3), Math.abs(fy - 3));
          on = ring === 3 || ring <= 1;
        } else if (nearFinder(x, y)) {
          on = false;
        } else {
          on = rnd() < 0.47;
        }
        if (on) d += "M" + x + " " + y + "h1v1h-1z";
      }
    }
    var svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("viewBox", "-1 -1 27 27");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    var path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("fill", "#062b8a");
    path.setAttribute("d", d);
    svg.appendChild(path);
    return svg;
  }
  Array.prototype.forEach.call(document.querySelectorAll(".qr"), function (el, i) {
    el.appendChild(qrSvg(0x9e3779b9 ^ Math.imul(i + 1, 2654435761)));
  });
})();
