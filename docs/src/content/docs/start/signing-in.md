---
title: Signing in
description: Connect your YouTube Music account to see your library and liked songs.
---

You can search and play music without signing in. Connect your account to
browse your library, see your liked songs, and like tracks from ytkew.

## From the browser

First, sign in at [music.youtube.com](https://music.youtube.com) in your browser.
Then tell ytkew which browser you use:

```sh
ytkew --auth browser brave
```

Replace `brave` with `chrome`, `chromium`, `firefox`, `librewolf`, `zen`,
`edge`, `vivaldi`, `opera`, `opera_gx`, or `arc`. Safari is supported on macOS.

If you leave out the name, ytkew looks for a Firefox login:

```sh
ytkew --auth browser
```

You don't need to paste a cookie. ytkew reads the browser's saved session,
checks that you're signed in, and saves it for the next time you open the player.
If the check fails, it keeps your previous login.

Chrome, Brave, and other Chromium-based browsers may need your desktop keyring
to be unlocked. If ytkew can't read the cookies, close the browser and try
again. You can also use the manual method below.

Once setup succeeds, start the player:

```sh
ytkew
```

Cookies can expire. If your library stops loading, sign in again in your
browser and rerun the same auth command.

## Pasting a header

If automatic login doesn't work with your browser:

1. Open music.youtube.com and make sure you're signed in.
2. Open Developer Tools, select **Network**, and reload the page.
3. Select a request to `music.youtube.com` and find **Cookie** under its request headers.
4. Run `ytkew --auth cookie`, paste the Cookie value when prompted, and press Enter.

Paste only the value, without `Cookie:` or surrounding quotes. ytkew checks
the session before saving it.

To check what ytkew can access, run:

```sh
ytkew --diagnose
```

The output shows whether ytkew loaded credentials and whether it can read
your library. It doesn't print the cookies themselves.

## OAuth

```sh
ytkew --auth oauth
```

This needs a Google Cloud OAuth client of type *TVs and Limited Input devices*.
You'll be asked for its client ID and secret, then given a link to finish
signing in. Most users should use browser login instead.

:::danger
Your cookies give access to your account. ytkew stores them in
`~/.config/ytkew/cookie.txt`, readable only by your user. Don't paste cookies
into chat, bug reports, or screenshots.
:::
