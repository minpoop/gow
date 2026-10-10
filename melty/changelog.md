Second alpha. Fixes the first test run, where the game never appeared:

- The launcher now logs every step (and ModEngine2's own output) to `%LOCALAPPDATA%\GowRing\logs\launcher.log`, finds ModEngine2 wherever Melty put it, and can no longer be held up by a slow scan of your God of War Ragnarök folder (the setup step is time-boxed).
- The launcher and mod no longer need the Visual C++ runtime installed.
- The package now lives in its own `GowRing/` folder so it cannot clash with other Elden Ring mashups.
- The mod reports its own start-up checks to Melty's test harness (loaded, save redirected, params patched, overlay hooked).

Still an alpha that has never run inside the real games — see STATUS.md.
