# Security policy

Signet loads untrusted web pages and stores sensitive data (passwords, browsing history, the keys that pair your phone). Security reports are taken seriously.

## Reporting a vulnerability

**Please don't open a public issue for a security problem.**

Report it privately through GitHub: go to the repository's [Security tab](https://github.com/ItsKarmaOff/Signet/security) and click **Report a vulnerability**. Only the maintainers can see the report.

Please include:

- what the issue is and what an attacker could do with it;
- the steps or a proof of concept to reproduce it;
- the affected version or commit, and your system (macOS, Linux, Windows, iOS, Android).

You'll get a first answer within a week. Once the issue is fixed, it will be disclosed in the release notes, with credit to you unless you prefer otherwise.

## Scope

In scope, for example:

- a web page reaching Signet's interface or its internal APIs (`window.signet`, IPC channels, `signetui://`, `signet://` pages);
- a page script (password autofill, swipe navigation, notes) being abused by a site;
- saved passwords, backups or pairing keys readable without the system keychain;
- the desktop ↔ phone link letting an unpaired device connect or read data;
- extensions, mods or Boosts escaping the limits they're supposed to have.

Out of scope:

- vulnerabilities in Chromium or Electron themselves, unless Signet's configuration makes them worse (report those upstream);
- issues that need an attacker who already controls your computer or your unlocked phone;
- the unsigned test builds being flagged by macOS Gatekeeper or Windows SmartScreen.

## Supported versions

Signet has no stable release yet. Fixes go to the `main` branch and the next build.
