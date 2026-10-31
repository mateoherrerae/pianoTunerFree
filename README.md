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
  las 3, y los pasos se adaptan.
- **Varios pianos guardados**, cada uno con su La, sus cuerdas y su curva.
- **Niveles Básico / Intermedio / Avanzado**, o cada ayuda por separado
  (recordatorio de apagar cuerdas, explicaciones, avance automático).
- **Detector de batido en los unísonos**: te dice cuántos batidos por segundo
  hay y cuándo el unísono queda limpio (puede avanzar solo).
- **Medir mi piano**: tocás 10 teclas y calcula el estiramiento propio de tu
  piano a partir de su inharmonicidad.
- **Graves por armónicos**: debajo de C3 mide con los armónicos, porque los
  micrófonos de notebook casi no captan la nota base.
- **Mantiene la última lectura** unos segundos, para los agudos cortos.
- **Elección de micrófono** con medidor de volumen.
- **Retomar** una afinación a la mitad, teclado clickeable, español e inglés.

## Próximamente

- Entrada MIDI (teclado digital como referencia o control).
- Subida de afinación en dos pasadas para pianos muy bajos.

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
