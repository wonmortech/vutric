// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
// Runs before first paint: applies the visitor's saved theme so the page never flashes the wrong one.
(function () {
  var theme = null;
  try { theme = window.localStorage.getItem("vutric-theme"); } catch (e) { theme = null; }
  if (theme === "light" || theme === "dark") document.documentElement.setAttribute("data-theme", theme);
  document.documentElement.classList.add("js");
})();
