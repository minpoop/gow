GowRProbe 0.3 — Kratos in the Lands Between, live-Kratos test 3: finding Ragnarök's camera
==========================================================================================

What it is
  A test for the "Ragnarök draws Kratos inside Elden Ring" idea. It loads a probe into God of War Ragnarök that
  watches how the game draws (frame rate, video memory, screenshots) and, when you press Ctrl+F9, searches
  Ragnarök's memory for its camera while you turn it, guided by beeps. While Ragnarök is not the window in front,
  it keeps Ragnarök from reading an Xbox-style controller (so its Kratos stays still) and holds it to 30 frames a
  second (so Elden Ring has more room). It changes no files of either game and no saves. It is separate from the
  mod; the mod keeps working as before.

How to run it
  1. Unzip this folder anywhere (for example your Downloads folder).
  2. Start Elden Ring through Melty as usual if you want both running, and God of War Ragnarök from Steam.
     Load a save and get into the game itself, with Kratos somewhere calm and no enemies around.
  3. Double-click GowRProbe.exe. It finds Ragnarök and loads the probe. Leave its window open.
     (Windows may warn about an unknown program: "More info" -> "Run anyway". If it says access was refused,
     right-click GowRProbe.exe -> "Run as administrator".)
  4. In Ragnarök, press Ctrl+F9 and follow the beeps:
       one beep        let go of the controller and keep completely still (15-20 seconds): it searches memory,
                       then watches for a second which part of Ragnarök writes the camera
       two beeps       move the camera: turn it slowly all the way round, look up and down, walk a few steps
       three beeps     done (about 10 seconds later). Once more somewhere else if you can.
  5. Put Elden Ring in front and play it for a minute, then close Ragnarök.
  6. A folder opens. Send Claude everything in it: probe.log, camsearch_*.bin, shot_*.png. If the .bin files won't
     attach, right-click the folder -> Send to -> Compressed (zipped) folder, and send the zip.
     (The folder is %LOCALAPPDATA%\GowRing\gowr_probe\latest; the run before is kept in "previous".)

If anything crashes or freezes, close Ragnarök; the log is still written up to that point.
