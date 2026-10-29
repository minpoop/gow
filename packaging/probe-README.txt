GowRProbe 0.5 — Kratos in the Lands Between, live-Kratos test 5: moving Ragnarök's camera
=====================================================================================

What it is
  A test for the "Ragnarök draws Kratos inside Elden Ring" idea. It loads a probe into God of War Ragnarök that
  watches how the game draws (frame rate, video memory, screenshots) and, when you press Ctrl+F9, tries three
  ways of raising Ragnarök's camera by 2 metres, 3 seconds each, guided by beeps, to learn which one Ragnarök
  really draws from. While Ragnarök is not the window in front,
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
       one beep        let go of the controller and keep completely still, about 20 seconds. Three times, for
                       3 seconds each, the view may jump up by 2 metres and come back: note which times it did
                       (the first, second, third, or none)
       three beeps     done. Once more somewhere else if you can.
  5. Put Elden Ring in front and play it for a minute, then close Ragnarök.
  6. A folder opens. Send Claude everything in it: probe.log, camsearch_*.bin, shot_*.png. If the .bin files won't
     attach, right-click the folder -> Send to -> Compressed (zipped) folder, and send the zip.
     (The folder is %LOCALAPPDATA%\GowRing\gowr_probe\latest; the run before is kept in "previous".)

If anything crashes or freezes, close Ragnarök; the log is still written up to that point.
