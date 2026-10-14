Fifty-fifth alpha: recording how Elden Ring really poses your character, to fix the shoulders properly.

0.1.53 passed its test. The hair on top of Kratos's head is gone; some still shows at the back and sides in the character creator, and a shoulder still comes through the cloak there.

- **Pose recording (for fixing the shoulders).** Every shoulder fix so far was tested against a pose I made up, and none of them matched the game's real idle stance. This version records the character's real bone positions 10, 20 and 40 seconds after you enter the world, into `er_pose_10s.json`, `er_pose_20s.json` and `er_pose_40s.json` in your `%LOCALAPPDATA%\GowRing` folder. The cloak can then be fitted against the real pose.
- **Hairstyle number.** The log also notes your character's face settings, to find which hairstyle number to switch to "none" so no hair can show under Kratos's head.
- Kratos's model is the same as 0.1.53, so nothing rebuilds.
