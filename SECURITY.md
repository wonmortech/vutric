# Security policy

## Supported versions

Vutric is in Phase 0 (technical spikes). There are no releases yet; security fixes go to `main`. When releases
exist, the latest release receives fixes.

## Reporting a vulnerability

Please **do not** open a public issue, pull request or discussion for a vulnerability.

1. Preferred: GitHub private vulnerability reporting, in this repository's **Security** tab, then **Report a
   vulnerability**: <https://github.com/wonmortech/vutric/security/advisories/new>.
2. If you cannot use GitHub: a private message on Telegram to <https://t.me/xnosir>. A short first message such as
   "Security report for Vutric" is enough; we will agree on a safe way to share the details.

Include the affected file or component, the steps to reproduce, and what an attacker could gain.

## What is in scope

Vutric accepts files from another device, so these matter most:

- a peer that is not paired being able to connect or send anything;
- a paired peer writing outside the save folder (file-name handling), exhausting memory or disk, or crashing the
  desktop app with a crafted message;
- the overlay taking keyboard focus or capturing input that belongs to other applications.

## What happens next

| Step | What we do | Within |
|---|---|---|
| Acknowledge | Confirm we received your report | 7 days |
| Assess | Reproduce it and rate its severity (CVSS) | 14 days |
| Fix | Fix high and critical issues, sooner where possible | 60 days |
| Disclose | Publish a GitHub security advisory (and a CVE where needed) with the fix | With the fix |

We credit reporters in the advisory if they wish. Good-faith research that follows the
[organization security policy](https://github.com/wonmortech/.github/blob/main/SECURITY.md) is welcome and covered
by its safe-harbour section. There is no paid bug bounty.
