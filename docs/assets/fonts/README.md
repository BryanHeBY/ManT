# Screenshot fonts

The reader screenshot uses the static JetBrains Mono 2.304 Regular, Bold,
Italic, and Bold Italic TrueType files from the
[JetBrains Mono v2.304 release](https://github.com/JetBrains/JetBrainsMono/tree/v2.304).
They are kept in the repository so local and CI captures do not depend on the
host's font packages. `fonts.conf` limits Fontconfig discovery to this
directory and fixes grayscale antialiasing and hinting for the capture.

The font files are licensed separately from ManT under the SIL Open Font
License 1.1. See [OFL.txt](OFL.txt).

| File | SHA-256 |
| --- | --- |
| `JetBrainsMono-Regular.ttf` | `a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f` |
| `JetBrainsMono-Bold.ttf` | `5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb` |
| `JetBrainsMono-Italic.ttf` | `9d0a1f7a708e6af183f1193b7e81d40da294f5c67682c085d8401c60aac8ded4` |
| `JetBrainsMono-BoldItalic.ttf` | `4039d5ce0ed225bf9c8b2c8c6436290ae2f356b7e90d70fa666227238324aa3b` |
