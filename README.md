# Piano Tuner Free

Afinador de piano **gratuito y open source** para Windows, con interfaz gráfica.
Usa el micrófono de tu compu y te guía nota por nota, cuerda por cuerda.

## Descargar

Bajá `PianoTunerFree.exe` desde [Releases](https://github.com/mateoherrerae/pianoTunerFree/releases)
y abrilo con doble clic. No necesita instalación.

## Qué hace

- **La central configurable**: 440, 442 o lo que quieras, por ejemplo 417 Hz
  para un piano viejo que conviene no subir de golpe.
- **Cuerdas de tu piano**: elegís en qué nota empiezan las 2 cuerdas y en cuál
  las 3. El programa adapta los pasos (apagar cuerdas de los costados, afinar
  la del medio, después las demás sin batido).
- **Afinación guiada de las 88 notas**, empezando por la octava central.
- **Afinador libre**: reconoce cualquier nota que toques.
- **Estiramiento (Railsback)**: agudos un poco más altos y graves un poco más
  bajos, como hacen los afinadores profesionales.
- **Mantiene la última lectura** unos segundos: útil para los agudos que se
  apagan enseguida.
- **Escuchar la nota**: reproduce el tono objetivo por los parlantes.
- Teclado clickeable para saltar a cualquier nota.

## Consejos de micrófono (notebooks)

Desactivá las "mejoras de audio", la cancelación de ruido (Windows, Lenovo
Vantage, AMD/NVIDIA) y poné el volumen del micrófono alto. Esos filtros se
comen los agudos cortos.

## Compilar desde el código

Necesitás [Rust](https://rustup.rs).

```
cargo build --release
```

El ejecutable queda en `target/release/PianoTunerFree.exe`.

## Créditos

El motor de detección de tono, inharmonicidad y orden de afinación viene de
[pianito](https://github.com/4esv/pianito) de Axel Stevens (MIT). Este
proyecto le agrega la interfaz gráfica, la configuración de cuerdas por nota y
otros cambios.

## Licencia

MIT. Ver [LICENSE](LICENSE).
