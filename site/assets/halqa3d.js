// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
//
// Halqa, in WebGL: the hero signature. A volumetric portal (glow lens with depth, ripples and a
// Signal->Glow rim) and a photo card that flies the throw path in 2.5D, reacting to the cursor.
// Same timeline as crates/vutric-overlay/src/timeline.rs and assets/vutric.js
// (travel 1.05 s, hold 1.4 s, dock 0.45 s; back-ease open, cubic-Bezier flight, 3.2 Hz spring land).
//
// Progressive enhancement: if WebGL is missing it does nothing and the Canvas-2D Halqa in vutric.js
// keeps running. prefers-reduced-motion renders one still frame and stops.
"use strict";

(function () {
  var hero = document.querySelector(".hero");
  if (!hero) return;
  var canvas2d = hero.querySelector('[data-halqa="hero"]');
  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // ---------- make a GL context, or bail to the 2D effect ----------
  var glCanvas = document.createElement("canvas");
  glCanvas.className = "hero__gl";
  glCanvas.setAttribute("aria-hidden", "true");
  var gl = null;
  try {
    var attrs = { alpha: true, premultipliedAlpha: false, antialias: true, depth: false, powerPreference: "high-performance" };
    gl = glCanvas.getContext("webgl", attrs) || glCanvas.getContext("experimental-webgl", attrs);
  } catch (e) { gl = null; }
  if (!gl) return; // 2D Halqa stays

  // ---------- easing / path (ported from vutric.js, kept identical) ----------
  function clamp01(x) { return x < 0 ? 0 : x > 1 ? 1 : x; }
  function easeOutCubic(x) { var k = 1 - x; return 1 - k * k * k; }
  function easeInCubic(x) { return x * x * x; }
  function easeBackOut(x) { var c1 = 1.6, c3 = c1 + 1, k = x - 1; return 1 + c3 * k * k * k + c1 * k * k; }
  function spring(tt) { var w = 2 * Math.PI * 3.2, z = 0.42, wd = w * Math.sqrt(1 - z * z); return Math.exp(-z * w * tt) * Math.sin(wd * tt); }
  function bezier(p0, p1, p2, p3, tt) {
    var u = 1 - tt, a = u * u * u, b = 3 * u * u * tt, c = 3 * u * tt * tt, d = tt * tt * tt;
    return [a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0], a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]];
  }
  var CFG = { travel: 1.05, hold: 1.4, dock: 0.45, gap: 0.9 };
  var TOTAL = CFG.travel + CFG.hold + CFG.dock + CFG.gap;
  var STILL = 0.42;

  // ---------- shaders ----------
  var VERT = [
    "attribute vec2 aPos;",
    "attribute vec2 aUv;",
    "varying vec2 vUv;",
    "uniform vec2 uRes;",
    "void main(){",
    "  vUv = aUv;",
    "  vec2 clip = (aPos / uRes) * 2.0 - 1.0;",
    "  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);",
    "}"
  ].join("\n");

  // Portal: a screen-covering quad, additive. Depth comes from a dark lens with inward ripples and a
  // faint swirl; the rim is a bloomed torus whose hue runs Signal<->Glow around the ring.
  var PORTAL_FS = [
    "precision highp float;",
    "uniform vec2 uCenter;",   // portal center, framebuffer px
    "uniform vec2 uRadii;",    // rx, ry px (already scaled by open/close)
    "uniform float uTime;",
    "uniform float uAlpha;",   // overall portal alpha
    "uniform float uOpen;",    // 0..1 opening wipe
    "const vec3 SIGNAL = vec3(0.31, 0.549, 1.0);",   // #4f8cff blue
    "const vec3 CYAN   = vec3(0.204, 0.878, 0.941);",// #34e0f0
    "const vec3 VIOLET = vec3(0.604, 0.420, 1.0);",  // #9a6bff
    "const vec3 GOLD   = vec3(1.0, 0.788, 0.471);",  // #ffc978 hot core
    "const vec3 NIGHT  = vec3(0.024, 0.027, 0.075);",// #060713
    "float hash(vec2 p){ return fract(sin(dot(p, vec2(41.3, 289.1))) * 43758.5453); }",
    "float noise(vec2 p){ vec2 i=floor(p), f=fract(p); f=f*f*(3.0-2.0*f);",
    "  float a=hash(i), b=hash(i+vec2(1.0,0.0)), c=hash(i+vec2(0.0,1.0)), d=hash(i+vec2(1.0,1.0));",
    "  return mix(mix(a,b,f.x), mix(c,d,f.x), f.y); }",
    "float fbm(vec2 p){ float s=0.0, a=0.5; for(int i=0;i<4;i++){ s+=a*noise(p); p*=2.03; a*=0.5; } return s; }",
    "void main(){",
    "  vec2 d = (gl_FragCoord.xy - uCenter) / uRadii;",   // elliptical space; |d|=1 at the rim
    "  float r = length(d);",
    "  float ang = atan(d.y, d.x);",
    "  vec3 col = vec3(0.0); float a = 0.0;",
    "  if (r < 1.02) {",                                  // inner lens (depth)
    "    float depth = 1.0 - smoothstep(0.0, 1.0, r);",
    "    float rip = 0.5 + 0.5 * sin(r * 26.0 - uTime * 2.2 + fbm(d * 2.5 + uTime * 0.15) * 3.0);",
    "    float swirl = fbm(vec2(ang * 1.6 + uTime * 0.25, r * 2.2 - uTime * 0.2));",
    "    vec3 lens = mix(NIGHT * 0.25, SIGNAL * 0.5, depth * (0.35 + 0.4 * rip));",
    "    lens += SIGNAL * swirl * depth * 0.35;",
    "    float inner = 1.0 - smoothstep(0.86, 1.02, r);",
    "    col += lens * inner;",
    "    a = max(a, inner * (0.55 + 0.45 * depth));",
    "  }",
    "  float rim = exp(-pow((r - 1.0) * 7.0, 2.0));",     // tight bright rim
    "  float halo = exp(-pow((r - 1.0) * 2.1, 2.0));",    // wide bloom
    "  float hue = 0.5 + 0.5 * sin(ang * 2.0 + uTime * 0.6);",
    "  float hue2 = 0.5 + 0.5 * sin(ang * 3.0 - uTime * 0.4);",
    "  vec3 rimCol = mix(mix(SIGNAL, CYAN, hue), VIOLET, hue2 * 0.5);",
    "  vec3 core = mix(rimCol, GOLD, rim * 0.8);",
    "  col += core * rim * 1.8;",
    "  col += rimCol * halo * 0.6;",
    "  a = max(a, rim * 1.1 + halo * 0.45);",
    "  a *= uAlpha * smoothstep(0.0, 0.5, uOpen);",
    "  col *= uAlpha;",
    "  gl_FragColor = vec4(col, clamp(a, 0.0, 1.0));",
    "}"
  ].join("\n");

  // Card: a textured quad (corners built in JS → 2.5D tilt) with a Glow rim and a diagonal sheen.
  var CARD_FS = [
    "precision mediump float;",
    "varying vec2 vUv;",
    "uniform sampler2D uTex;",
    "uniform float uAlpha;",
    "uniform float uSheen;",
    "void main(){",
    "  vec2 uv = vUv;",
    "  vec4 tex = texture2D(uTex, uv);",
    "  float edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));",
    "  float rim = smoothstep(0.045, 0.0, edge);",         // bright inner border
    "  vec3 col = mix(tex.rgb, vec3(0.957, 0.851, 0.651), rim * 0.6);",
    "  float sheen = smoothstep(0.75, 0.0, abs(uv.x + uv.y - 1.0 - uSheen));",
    "  col += vec3(1.0) * sheen * 0.12;",
    "  float aa = smoothstep(0.0, 0.01, edge);",           // soft antialiased corners
    "  gl_FragColor = vec4(col, uAlpha * aa);",
    "}"
  ].join("\n");

  function compile(type, src) {
    var s = gl.createShader(type);
    gl.shaderSource(s, src); gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) { gl.deleteShader(s); return null; }
    return s;
  }
  function program(vsSrc, fsSrc) {
    var vs = compile(gl.VERTEX_SHADER, vsSrc), fs = compile(gl.FRAGMENT_SHADER, fsSrc);
    if (!vs || !fs) return null;
    var p = gl.createProgram();
    gl.attachShader(p, vs); gl.attachShader(p, fs); gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) return null;
    return p;
  }
  var portalProg = program(VERT, PORTAL_FS);
  var cardProg = program(VERT, CARD_FS);
  if (!portalProg || !cardProg) return; // driver rejected the shaders → 2D stays

  function aloc(p, n) { return gl.getAttribLocation(p, n); }
  function uloc(p, n) { return gl.getUniformLocation(p, n); }
  var PA = { pos: aloc(portalProg, "aPos"), uv: aloc(portalProg, "aUv") };
  var PU = { res: uloc(portalProg, "uRes"), center: uloc(portalProg, "uCenter"), radii: uloc(portalProg, "uRadii"), time: uloc(portalProg, "uTime"), alpha: uloc(portalProg, "uAlpha"), open: uloc(portalProg, "uOpen") };
  var CA = { pos: aloc(cardProg, "aPos"), uv: aloc(cardProg, "aUv") };
  var CU = { res: uloc(cardProg, "uRes"), tex: uloc(cardProg, "uTex"), alpha: uloc(cardProg, "uAlpha"), sheen: uloc(cardProg, "uSheen") };

  var quadBuf = gl.createBuffer();
  var cardBuf = gl.createBuffer();

  // ---------- the photo (procedural dusk landscape; same look as vutric.js) ----------
  function makePhoto() {
    var c = document.createElement("canvas"); c.width = 480; c.height = 360;
    var g = c.getContext("2d");
    var sky = g.createLinearGradient(0, 0, 0, 360);
    sky.addColorStop(0, "#22336f"); sky.addColorStop(0.45, "#7a5a8a"); sky.addColorStop(0.62, "#e08a62"); sky.addColorStop(1, "#f6c98a");
    g.fillStyle = sky; g.fillRect(0, 0, 480, 360);
    var sun = g.createRadialGradient(330, 205, 4, 330, 205, 120);
    sun.addColorStop(0, "rgba(255,240,200,1)"); sun.addColorStop(0.18, "rgba(255,214,150,0.9)"); sun.addColorStop(1, "rgba(255,190,120,0)");
    g.fillStyle = sun; g.fillRect(0, 0, 480, 360);
    var hills = ["#5a3f68", "#3a2d58", "#241f45", "#151634"];
    for (var i = 0; i < hills.length; i++) {
      g.fillStyle = hills[i]; g.beginPath(); g.moveTo(0, 360);
      for (var x = 0; x <= 480; x += 8) { g.lineTo(x, 360 * (0.58 + 0.09 * i) + 13 * Math.sin(x * (0.012 + 0.006 * i) + i * 1.7) + 6 * Math.cos(x * 0.045 + i)); }
      g.lineTo(480, 360); g.closePath(); g.fill();
    }
    return c;
  }
  var tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, makePhoto());
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

  // ---------- state ----------
  var W = 0, H = 0, dpr = 1;
  var t = reduceMotion ? STILL : 0;
  var playing = !reduceMotion;
  var visible = true;
  var raf = 0, last = 0;
  var mouse = { x: 0, y: 0, tx: 0, ty: 0 }; // target + smoothed

  function px(x) { return x * dpr; }

  function resize() {
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    var rect = hero.getBoundingClientRect();
    W = Math.max(1, Math.round(rect.width));
    H = Math.max(1, Math.round(rect.height));
    glCanvas.width = Math.round(W * dpr);
    glCanvas.height = Math.round(H * dpr);
    glCanvas.style.width = W + "px";
    glCanvas.style.height = H + "px";
    gl.viewport(0, 0, glCanvas.width, glCanvas.height);
  }

  function layout() {
    var narrow = W < 768;
    // Keep the portal and card in the upper-right, clear of the bottom-aligned headline.
    var ry = Math.min(H * (narrow ? 0.15 : 0.32), narrow ? 150 : 300);
    var rx = ry * 0.92;
    var ring = narrow ? [W * 0.5, H * 0.15] : [W * 0.84, H * 0.36];
    var land = narrow ? [W * 0.48, H * 0.17] : [W * 0.82, H * 0.42];
    var cardH = Math.min(H * (narrow ? 0.14 : 0.32), W * (narrow ? 0.4 : 0.24), narrow ? 130 : 290), cardW = cardH * 4 / 3;
    return { ring: ring, rx: rx, ry: ry, land: land, dock: [W - 42, H - 42], cardW: cardW, cardH: cardH };
  }

  // Sample the timeline (mirrors Halqa.sample in vutric.js).
  function sample(tt) {
    var L = layout(), travel = CFG.travel, u = tt / travel;
    var open = clamp01(u / 0.24), close = clamp01((u - 0.6) / 0.3);
    var ring = { scale: easeBackOut(open) * (1 - 0.35 * easeInCubic(close)), alpha: clamp01(open * 3) * (1 - close) };
    var f = clamp01((u - 0.10) / 0.52), e = easeOutCubic(f);
    var p0 = L.ring, p3 = L.land, dx = p0[0] - p3[0];
    var p1 = [p0[0] - dx * 0.25, p0[1] - L.ry * 0.55];
    var p2 = [p3[0] + dx * 0.2, p3[1] - L.ry * 0.12];
    var pos = bezier(p0, p1, p2, p3, e);
    var scale = 0.16 + 0.84 * e, rot = -0.42 * (1 - e), alpha = clamp01(f * 5);
    var tl = 0.62 * travel;
    if (tt > tl) { var s = spring(tt - tl); scale = 1 + 0.07 * s; rot = -0.05 * s; }
    var dockT = tt - travel - CFG.hold;
    if (dockT > 0) {
      var dmix = easeInCubic(clamp01(dockT / CFG.dock));
      pos = [pos[0] + (L.dock[0] - pos[0]) * dmix, pos[1] + (L.dock[1] - pos[1]) * dmix];
      scale *= 1 - 0.78 * dmix; alpha *= 1 - dmix;
    }
    return { L: L, ring: ring, card: { pos: pos, scale: scale, rot: rot, alpha: alpha }, u: u };
  }

  function bindQuad(attrs, verts, buf) {
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, verts, gl.DYNAMIC_DRAW);
    gl.enableVertexAttribArray(attrs.pos);
    gl.vertexAttribPointer(attrs.pos, 2, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(attrs.uv);
    gl.vertexAttribPointer(attrs.uv, 2, gl.FLOAT, false, 16, 8);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
  }

  function render() {
    var S = sample(t), L = S.L;
    mouse.x += (mouse.tx - mouse.x) * 0.06;
    mouse.y += (mouse.ty - mouse.y) * 0.06;
    var par = { x: mouse.x, y: mouse.y };

    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.enable(gl.BLEND);

    // portal — additive, screen-covering quad (positions & uRes both in framebuffer px)
    if (S.ring.alpha > 0.002) {
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
      gl.useProgram(portalProg);
      gl.uniform2f(PU.res, px(W), px(H));
      gl.uniform2f(PU.center, px(L.ring[0] + par.x * 34), px(H) - px(L.ring[1] + par.y * 22)); // flip Y for gl_FragCoord
      gl.uniform2f(PU.radii, px(L.rx * S.ring.scale), px(L.ry * S.ring.scale));
      gl.uniform1f(PU.time, t);
      gl.uniform1f(PU.alpha, S.ring.alpha);
      gl.uniform1f(PU.open, clamp01(S.u / 0.24));
      var W2 = px(W), H2 = px(H);
      bindQuad(PA, new Float32Array([0, 0, 0, 0, W2, 0, 1, 0, 0, H2, 0, 1, 0, H2, 0, 1, W2, 0, 1, 0, W2, H2, 1, 1]), quadBuf);
    }

    // card — normal alpha, corners built in JS for a 2.5D cursor tilt
    var c = S.card;
    if (c.alpha > 0.002) {
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
      gl.useProgram(cardProg);
      gl.uniform2f(CU.res, px(W), px(H));
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.uniform1i(CU.tex, 0);
      gl.uniform1f(CU.alpha, c.alpha);
      gl.uniform1f(CU.sheen, (t * 0.5) % 2.0 - 1.0);
      var cx = px(c.pos[0] + par.x * 26), cy = px(c.pos[1] + par.y * 18);
      var hw = px(L.cardW * c.scale * 0.5), hh = px(L.cardH * c.scale * 0.5);
      var cr = Math.cos(c.rot), sr = Math.sin(c.rot);
      var tiltX = par.x * 0.16, tiltY = par.y * 0.12;
      function corner(sx, sy) {
        var fx = 1 + tiltX * sx, fy = 1 + tiltY * sy;
        var lx = sx * hw * fy, ly = sy * hh * fx;
        return [cx + lx * cr - ly * sr, cy + lx * sr + ly * cr];
      }
      var a = corner(-1, -1), b = corner(1, -1), d2 = corner(-1, 1), e2 = corner(1, 1);
      bindQuad(CA, new Float32Array([a[0], a[1], 0, 0, b[0], b[1], 1, 0, d2[0], d2[1], 0, 1, d2[0], d2[1], 0, 1, b[0], b[1], 1, 0, e2[0], e2[1], 1, 1]), cardBuf);
    }
  }

  function frame(now) {
    raf = 0;
    if (!playing || !visible || document.hidden) return;
    var dt = last ? Math.min((now - last) / 1000, 0.05) : 0;
    last = now;
    t = (t + dt) % TOTAL;
    render();
    raf = requestAnimationFrame(frame);
  }
  function kick() { if (playing && visible && !document.hidden && !raf) { last = 0; raf = requestAnimationFrame(frame); } }
  function setPlaying(on) { playing = on; if (on) kick(); else render(); }

  // Minimal control hook (QA / reduced-motion tooling); no storage, no network.
  window.__halqa3d = { seek: function (x) { t = ((x % TOTAL) + TOTAL) % TOTAL; playing = false; render(); }, play: setPlaying };

  // ---------- mount: take over the hero from the 2D canvas ----------
  if (canvas2d) { canvas2d.style.display = "none"; } // its IntersectionObserver then pauses it
  hero.classList.add("hero--gl");
  hero.insertBefore(glCanvas, hero.firstChild); // backdrop below .hero__inner (z-index:1)

  resize();
  if (reduceMotion) { render(); } else { kick(); }

  if ("ResizeObserver" in window) { new ResizeObserver(function () { resize(); render(); }).observe(hero); }
  else { window.addEventListener("resize", function () { resize(); render(); }); }
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(function (es) { visible = es[0].isIntersecting; if (visible) kick(); }, { threshold: 0.02 }).observe(glCanvas);
  }
  document.addEventListener("visibilitychange", function () { if (!document.hidden) kick(); });

  if (!reduceMotion) {
    hero.addEventListener("pointermove", function (ev) {
      var rect = hero.getBoundingClientRect();
      mouse.tx = ((ev.clientX - rect.left) / rect.width) * 2 - 1;
      mouse.ty = ((ev.clientY - rect.top) / rect.height) * 2 - 1;
      kick();
    });
    hero.addEventListener("pointerleave", function () { mouse.tx = 0; mouse.ty = 0; });
  }

  // Rebind the existing hero pause button from the 2D effect to this one.
  var pause = hero.querySelector('[data-halqa-toggle="hero"]');
  if (pause) {
    var fresh = pause.cloneNode(true); // drops vutric.js's listener
    pause.parentNode.replaceChild(fresh, pause);
    fresh.hidden = false;
    var sync = function () {
      fresh.setAttribute("aria-pressed", String(!playing));
      fresh.textContent = playing ? "Pause animation" : "Play animation";
    };
    sync();
    fresh.addEventListener("click", function () { setPlaying(!playing); sync(); });
  }
})();
