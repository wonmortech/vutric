/* SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors */
/* SPDX-License-Identifier: Apache-2.0 */
/* Vutric landing motion: reveals, scroll progress, the connection diagram. Lean, no deps. */
(function () {
  "use strict";
  var reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* scroll progress + sticky nav */
  var prog = document.querySelector(".prog");
  var nav = document.querySelector(".nav");
  function onScroll() {
    var h = document.documentElement;
    var max = h.scrollHeight - h.clientHeight;
    var p = max > 0 ? h.scrollTop / max : 0;
    if (prog) prog.style.transform = "scaleX(" + p.toFixed(4) + ")";
    if (nav) nav.classList.toggle("is-stuck", h.scrollTop > 10);
  }
  addEventListener("scroll", onScroll, { passive: true });
  onScroll();

  /* reveals */
  var items = document.querySelectorAll("[data-reveal]");
  if (reduce || !("IntersectionObserver" in window)) {
    items.forEach(function (el) { el.classList.add("in"); });
  } else {
    var io = new IntersectionObserver(function (es) {
      es.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
      });
    }, { rootMargin: "0px 0px -12% 0px", threshold: 0.12 });
    items.forEach(function (el) { io.observe(el); });
  }

  /* hero pause button only matters when the WebGL hero mounted (window.__halqa3d) */
  setTimeout(function () {
    var btn = document.querySelector("[data-halqa-toggle='hero']");
    if (btn && !window.__halqa3d) btn.style.display = "none";
  }, 400);

  /* connection diagram: a pulse travels the path, the chain lights in step */
  var path = document.getElementById("dgPath");
  var pulse = document.querySelector(".dg-pulse");
  var flow = document.querySelector(".dg-flow");
  var steps = [].slice.call(document.querySelectorAll(".chain__step"));
  var connect = document.getElementById("connect");

  function lightStep(i) {
    steps.forEach(function (s, k) { s.classList.toggle("on", k === i); });
  }

  if (path && pulse && connect) {
    var len = path.getTotalLength();
    if (flow) { flow.style.strokeDasharray = len; }
    if (reduce) {
      var mid = path.getPointAtLength(len * 0.5);
      pulse.setAttribute("cx", mid.x); pulse.setAttribute("cy", mid.y);
      if (flow) flow.style.strokeDashoffset = 0;
      steps.forEach(function (s) { s.classList.add("on"); });
    } else {
      var running = false, t0 = 0;
      var DUR = 2600;
      function frame(now) {
        if (!running) return;
        if (!t0) t0 = now;
        var t = ((now - t0) % DUR) / DUR;       // 0..1 loop
        var pt = path.getPointAtLength(len * t);
        pulse.setAttribute("cx", pt.x.toFixed(1));
        pulse.setAttribute("cy", pt.y.toFixed(1));
        if (flow) flow.style.strokeDashoffset = (len * (1 - t)).toFixed(1);
        lightStep(Math.min(steps.length - 1, Math.floor(t * steps.length)));
        requestAnimationFrame(frame);
      }
      var vis = new IntersectionObserver(function (es) {
        es.forEach(function (e) {
          if (e.isIntersecting && !running) { running = true; t0 = 0; requestAnimationFrame(frame); }
          else if (!e.isIntersecting) { running = false; }
        });
      }, { threshold: 0.2 });
      vis.observe(connect);
    }
  }

  /* use-case tabs */
  var tabs = [].slice.call(document.querySelectorAll(".tab"));
  var panels = [].slice.call(document.querySelectorAll(".tabpanel"));
  tabs.forEach(function (t) {
    t.addEventListener("click", function () {
      var id = t.getAttribute("data-tab");
      tabs.forEach(function (x) { x.setAttribute("aria-selected", x === t ? "true" : "false"); });
      panels.forEach(function (p) { p.classList.toggle("on", p.getAttribute("data-panel") === id); });
    });
  });
})();
