# ADR-0055: The browser page rides on the LocalSend receiver

Status: accepted (2026-11-18) · Stream: W3 · Backlog: C2, F16 · Builds on: ADR-0010, ADR-0054

## Context

ADR-0010 promises a page on the LAN, reached by QR or link, so a device with no app (above all an iPhone,
because AirDrop is closed) can download what the user shares and upload to the user's computer.

## Decision

1. **Same server, same process.** The page is served by the LocalSend receiver of ADR-0054, inside the sandboxed
   adapter process, under `/s/<token>/`:
   - `/s/<token>/` the page, `/s/<token>/list` the shared files (JSON), `/s/<token>/f/<n>` one download;
   - only `GET`; anything else, a wrong token, or an expired one gets a bare `404`.
2. **Uploads use the LocalSend v2 API.** The page's script calls `prepare-upload` and `upload` like any LocalSend
   sender, with `deviceType: "web"`. So a browser upload is asked about, staged, rate-limited and size-checked by
   exactly the code a LocalSend app goes through; there is no second upload path to secure.
3. **Links:** `Receiver::share(paths, ttl)` / IPC `share` returns a 128-bit token; at most 16 live links, 1000
   files each, a day at most (the daemon passes 10 minutes, matching "everyone" in S3); `unshare` closes one at
   once. The daemon builds the QR from `https://<address>:<port>/s/<token>/` with the addresses of R1.
4. **Page hardening:**
   - one file, no external resources;
   - CSP `default-src 'none'` with a per-response nonce for the one script and the one stylesheet;
   - `Referrer-Policy: no-referrer` (the token is in the URL), `Cache-Control: no-store`, `nosniff`,
     `X-Frame-Options: DENY`;
   - downloads as `attachment` with an ASCII fallback name plus the exact UTF-8 name (RFC 6266, RFC 8187);
   - names rendered with `textContent` only.
   The text is Uzbek or English by the browser's language. It works at phone width and in both themes.
5. **HTTPS with the self-signed certificate**, as ADR-0010 says: the browser shows a warning once per
   certificate. Plain HTTP would avoid it, but it would put the files and the token on the LAN unencrypted. The
   trade-off is recorded for the owner.

## Measured

- `tests/browser.rs`: page headers, list, a 700 KB download byte for byte with the RFC 8187 name, wrong, other,
  closed and expired tokens, `POST` refused, and an upload offer shaped exactly like the page sends it.
- Real Chromium (Playwright 1.64.1, 390x844, light/English and dark/Uzbek):
  - download of a 2.5 MB file, name intact;
  - two files uploaded through the page, identical to the originals;
  - no console or CSP errors, no horizontal scroll.
  This run found one bug: the "Sent" message was cleared at once; fixed before the commit.

## Left open

- Real iPhone Safari and Android Chrome on a real LAN (with M5).
- Range requests (resuming a download) and zip-all for many files.
- Whether to offer plain HTTP as an opt-in (owner question).
