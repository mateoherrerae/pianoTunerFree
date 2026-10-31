# Piano Tuner Free 🎹

**Afinador de piano gratuito y open source para Windows.** Afiná tu piano con el micrófono de tu notebook: te guía nota por nota, cuerda por cuerda, con detector de batido para los unísonos.

🇬🇧 [Read in English](README.md)

## ⬇️ Descargar

**[Descargar PianoTunerFree.exe](https://github.com/mateoherrerae/pianoTunerFree/raw/main/dist/PianoTunerFree.exe)** (Windows 10/11, ~7 MB, no necesita instalación)

Abrilo con doble clic. Si Windows SmartScreen muestra un aviso, tocá **Más información → Ejecutar de todas formas** (la app no está firmada digitalmente).

## Qué hace

- **La central configurable**: 440, 442, 435 o cualquier valor, por ejemplo 417 Hz para un piano viejo que conviene no subir de golpe.
- **Cuerdas de tu piano**: elegís en qué nota empiezan las 2 cuerdas y en cuál las 3; los pasos se adaptan (apagar las de los costados, afinar la del medio, después los unísonos).
- **Detector de batido en los unísonos**: muestra los batidos por segundo y te avisa cuando el unísono queda limpio. Puede avanzar solo.
- **Medir mi piano**: tocás 10 teclas y la app calcula el estiramiento según la inharmonicidad de *tu* piano.
- **Graves por armónicos**: debajo de C3 mide con los armónicos, porque los micrófonos de notebook casi no captan la nota base.
- **Mantiene la última lectura** para los agudos que se apagan rápido.
- **Niveles**: Básico / Intermedio / Avanzado, o activás cada ayuda por separado.
- **Varios pianos guardados**, elección de micrófono con medidor de volumen y opción de retomar una afinación a la mitad.
- **Afinador libre** (cromático), teclado de 88 teclas clickeable y tono de referencia.
- Interfaz en **español e inglés**.

## Consejos de micrófono (notebooks)

Desactivá las "mejoras de audio" y la cancelación de ruido (configuración de sonido de Windows, Lenovo Vantage, software de AMD/NVIDIA) y poné el volumen del micrófono alto. Esos filtros se comen los agudos cortos.

## Compilar desde el código

Necesitás [Rust](https://rustup.rs).

```
cargo build --release
```

El ejecutable queda en `target/release/PianoTunerFree.exe`.

## Créditos

El motor de detección de tono, inharmonicidad y orden de afinación viene de [pianito](https://github.com/4esv/pianito) de Axel Stevens (MIT). Este proyecto le suma la interfaz gráfica, las cuerdas por piano, el detector de batido, la medición del piano y más.

## Licencia

MIT. Ver [LICENSE](LICENSE).

---

**Palabras clave:** afinador de piano, afinar piano, app para afinar piano, afinador de piano gratis, programa para afinar piano, afinador de piano para PC, afinador de piano Windows, afinador cromático, afinación de piano, unísonos, batido, inharmonicidad, estiramiento, curva de Railsback, La 417 Hz, La 440, diapasón, temperamento, afinar piano con micrófono, afinador open source, software libre.
