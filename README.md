# Piano Tuner Free 🎹

**Free, open-source piano tuning app for Windows.** Tune your piano with your laptop's microphone — guided note by note, string by string, with a beat detector for unisons.

🇪🇸 [Leer en español](README.es.md)

## ⬇️ Download

**[Download PianoTunerFree.exe](https://github.com/mateoherrerae/pianoTunerFree/raw/main/dist/PianoTunerFree.exe)** (Windows 10/11, ~7 MB, no installation needed)

Double-click to open. If Windows SmartScreen shows a warning, click **More info → Run anyway** (the app is not code-signed).

## Features

- **Custom pitch (A4)**: 440, 442, 435 or any value — e.g. 417 Hz for an old piano that shouldn't be raised all at once.
- **Your piano's string layout**: choose where 2-string and 3-string sections start; the steps adapt (mute outer strings, tune center, then unisons).
- **Unison beat detector**: shows beats per second and tells you when the unison is clean. Can auto-advance.
- **Measure my piano**: play 10 keys and the app computes a stretch curve from *your* piano's inharmonicity.
- **Bass by partials**: below C3 the pitch is measured from overtones, since laptop mics barely pick up the fundamental.
- **Holds the last reading** for short, fast-decaying treble notes.
- **Levels**: Basic / Intermediate / Advanced, or toggle each assist yourself.
- **Several saved pianos**, microphone selection with input meter, resume an unfinished tuning.
- **Free tuner** mode (chromatic), clickable 88-key keyboard, reference tone playback.
- **English and Spanish** interface.

## Microphone tips (laptops)

Turn off "audio enhancements" and noise cancellation (Windows sound settings, Lenovo Vantage, AMD/NVIDIA software) and set the mic volume high. Those filters cut short treble notes.

## Build from source

Requires [Rust](https://rustup.rs).

```
cargo build --release
```

The executable ends up in `target/release/PianoTunerFree.exe`.

## Credits

The pitch detection, inharmonicity and tuning-order engine comes from [pianito](https://github.com/4esv/pianito) by Axel Stevens (MIT). This project adds the graphical interface, per-piano string layout, beat detection, piano measurement and more.

## License

MIT — see [LICENSE](LICENSE).

---

**Keywords:** piano tuner, piano tuning app, free piano tuner, piano tuning software, open source piano tuner, Windows piano tuner, chromatic tuner, strobe tuner alternative, unison beat detector, inharmonicity, stretch tuning, Railsback curve, A4 417 Hz, A440, concert pitch, temperament, aural tuning, electronic tuning device, ETD, tune piano with microphone, laptop microphone tuner, Rust, egui.
