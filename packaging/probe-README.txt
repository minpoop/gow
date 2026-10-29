GowRProbe — Kratos in the Lands Between, live-Kratos test 1
=========================================================

What it is
  A small test for the "Ragnarök draws Kratos inside Elden Ring" idea. It loads a probe into God of War Ragnarök
  that only WATCHES how the game draws: frame rate in front of and behind other windows, video memory, the graphics
  set-up, and a few screenshots. While Ragnarök is not the window in front, it also stops Ragnarök from reading an
  Xbox-style controller, so its Kratos stays still while you play Elden Ring.
  It changes no files of either game and no saves. It is separate from the mod; the mod keeps working as before.

How to run it
  1. Unzip this folder anywhere (for example your Downloads folder).
  2. Start Elden Ring through Melty as usual if you want both running (recommended), and God of War Ragnarök from
     Steam. Load a save in Ragnarök somewhere calm.
  3. Double-click GowRProbe.exe. A window opens, finds Ragnarök and loads the probe. Leave the window open.
     (Windows may warn about an unknown program: choose "More info" -> "Run anyway". If it says access was refused,
     right-click GowRProbe.exe -> "Run as administrator".)
  4. Follow the steps it prints: play Ragnarök a minute, then put Elden Ring in front for 30 seconds or more, switch
     back once, then close Ragnarök.
  5. A folder opens. Send Claude probe.log and the shot_*.png pictures from it.
     (The folder is %LOCALAPPDATA%\GowRing\gowr_probe\latest; the run before is kept in "previous".)

If anything crashes or freezes, close Ragnarök; the log is still written up to that point.
