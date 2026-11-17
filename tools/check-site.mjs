// SPDX-FileCopyrightText: 2026 Wonmor and Vutric contributors
// SPDX-License-Identifier: Apache-2.0
// Static checks for site/ before every deploy (CI and deploy/deploy.sh ship). No dependencies.
// Fails on: inline scripts, styles or event handlers (the CSP forbids them), resources loaded from
// other origins, broken local links or anchors, server addresses, extra forbidden words and Cyrillic text.
import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { join, relative, dirname, resolve, sep } from "node:path";

const ROOT = resolve(process.argv[2] ?? "site");
const errors = [];
const fail = (file, msg) => errors.push(`${relative(process.cwd(), file)}: ${msg}`);

const walk = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
const files = walk(ROOT);
const text = files.filter((f) => /\.(html|css|js|svg|txt|xml)$/.test(f) && !f.endsWith("OFL.txt"));

const forbidden = [
  [/\b(?:\d{1,3}\.){3}\d{1,3}\b/, "an IPv4 address"],
  // Extra words that must never ship (names, internal hosts) stay out of the repo: CHECK_SITE_FORBID="a|b".
  ...(process.env.CHECK_SITE_FORBID ? [[new RegExp(process.env.CHECK_SITE_FORBID, "i"), "a forbidden word"]] : []),
  [/[Ѐ-ӿ]/, "Cyrillic text"],
  [/lorem ipsum/i, "a placeholder"],
  [/\bTODO\b|\bFIXME\b/, "a placeholder"],
];
for (const f of text) {
  const src = readFileSync(f, "utf8");
  for (const [re, what] of forbidden) if (re.test(src)) fail(f, `contains ${what}: ${src.match(re)[0]}`);
}

for (const f of files.filter((f) => f.endsWith(".html"))) {
  const html = readFileSync(f, "utf8");
  if (/<style[\s>]/i.test(html)) fail(f, "<style> element (CSP: style-src 'self')");
  if (/\sstyle\s*=/i.test(html)) fail(f, "style attribute (CSP: style-src 'self')");
  if (/\son[a-z]+\s*=/i.test(html)) fail(f, "inline event handler (CSP: script-src 'self')");
  for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
    if (!/\ssrc=/.test(m[1]) || m[2].trim()) fail(f, "inline <script> (CSP: script-src 'self')");
  }
  const ids = new Set([...html.matchAll(/\sid="([^"]+)"/g)].map((m) => m[1]));
  for (const m of html.matchAll(/<(link|script|img|source|use|a)\b[^>]*?\s(href|src)="([^"]+)"/gi)) {
    const [, tag, attr, url] = m;
    const loads = tag.toLowerCase() !== "a";
    if (/^(https?:)?\/\//i.test(url)) {
      if (loads && !/rel="canonical"/.test(m[0])) fail(f, `<${tag} ${attr}> loads from another origin: ${url}`);
      continue;
    }
    if (/^(mailto|tel|data):/i.test(url)) continue;
    if (url.startsWith("#")) {
      if (url.length > 1 && !ids.has(url.slice(1))) fail(f, `anchor ${url} has no matching id`);
      continue;
    }
    const path = url.split(/[?#]/)[0];
    const target = path.startsWith("/") ? join(ROOT, path) : join(dirname(f), path);
    const file = target.endsWith(sep) || target.endsWith("/") ? join(target, "index.html") : target;
    if (!existsSync(file)) fail(f, `${attr}="${url}" points to a missing file`);
  }
}

for (const need of ["index.html", "404.html", "favicon.svg", "robots.txt", "sitemap.xml"]) {
  if (!existsSync(join(ROOT, need))) errors.push(`site/${need} is missing`);
}

if (errors.length) {
  console.error(errors.map((e) => `  - ${e}`).join("\n"));
  console.error(`check-site: ${errors.length} problem(s)`);
  process.exit(1);
}
console.log(`check-site: ${files.length} files OK`);
